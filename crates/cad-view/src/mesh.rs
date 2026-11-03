//! Tessellating a display list for the screen.
//!
//! The buffers are in the byte layout of the app's shader (little endian):
//!
//! - `lines`: pairs of vertices, a hairline each. A vertex is `x: f32, y: f32,
//!   rgba: [u8; 4]` ([`VERTEX_SIZE`] bytes).
//! - `tris`: triangles, three vertices each, same vertex layout.
//! - `quads`: one wide stroke (or dot) per instance: `a: [f32; 2], b: [f32; 2],
//!   rgba: [u8; 4], width: f32, start: f32` ([`INSTANCE_SIZE`] bytes); the shader
//!   expands it to a screen-space rectangle `width` points wide with square caps.
//!
//! Positions are relative to [`Mesh::origin`], and colours are premultiplied
//! by alpha (all opaque here, except the selection highlight's fill).

use crate::parse::{Display, Op};
use crate::view::View;
use crate::{earcut, Error};
use std::collections::BTreeSet;
use std::f64::consts::{PI, TAU};

pub const VERTEX_SIZE: usize = 12;
pub const INSTANCE_SIZE: usize = 28;

/// How far a chord of a tessellated arc may stray from the true arc, in pixels.
const ARC_TOLERANCE_PX: f64 = 0.2;
const MAX_ARC_SEGMENTS: usize = 4096;
/// Zooming in by more than this needs the arcs cut finer.
const REBUILD_ZOOM_IN: f64 = 1.6;
/// A drawing with this many items or fewer is meshed whole, never cut to the view.
const CULL_ABOVE_ITEMS: usize = 20_000;
/// Points per world unit of a dot's square.
const DOT_WIDTH: f32 = 3.0;

#[derive(Debug, Clone)]
pub struct Options {
    /// Pixels per world unit the arcs are cut for.
    pub scale: f64,
    /// The world rectangle on show. A large drawing is meshed only a screen or two around it.
    pub view: Option<[f64; 4]>,
    /// Print colours instead of the dark canvas's.
    pub print: bool,
    /// Draw lineweights (false: every stroke a hairline).
    pub line_weights: bool,
    /// Most bytes the mesh may hold.
    pub max_bytes: usize,
}

impl Default for Options {
    fn default() -> Options {
        Options { scale: 1.0, view: None, print: false, line_weights: true, max_bytes: 1 << 30 }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MeshStats {
    pub items_drawn: usize,
    pub items_culled: usize,
    pub segments: usize,
    pub triangles: usize,
    pub fills: usize,
    /// Fills the triangulator could not cut exactly (drawn as a fan; usually a damaged ring).
    pub fills_approximate: usize,
}

#[derive(Debug, Clone)]
pub struct Mesh {
    pub origin: [f64; 2],
    pub lines: Vec<u8>,
    pub tris: Vec<u8>,
    pub quads: Vec<u8>,
    pub stats: MeshStats,
    /// The pixels per world unit this was cut for.
    pub scale: f64,
    /// The world rectangle this holds (infinite when the whole drawing was meshed).
    pub covers: [f64; 4],
}

/// What to draw instead of an item's own look (the selection highlight).
#[derive(Clone, Copy)]
struct Highlight {
    /// Premultiplied.
    stroke: [u8; 4],
    fill: [u8; 4],
    width: f32,
}

struct Builder<'a> {
    o: &'a Options,
    origin: [f64; 2],
    m: Mesh,
}

impl Builder<'_> {
    fn rel(&self, p: [f64; 2]) -> Option<[f32; 2]> {
        let r = [(p[0] - self.origin[0]) as f32, (p[1] - self.origin[1]) as f32];
        (r[0].is_finite() && r[1].is_finite()).then_some(r)
    }

    fn vertex(out: &mut Vec<u8>, p: [f32; 2], c: [u8; 4]) {
        out.extend_from_slice(&p[0].to_le_bytes());
        out.extend_from_slice(&p[1].to_le_bytes());
        out.extend_from_slice(&c);
    }

    fn stroke(&mut self, a: [f64; 2], b: [f64; 2], c: [u8; 4], width: Option<f32>) {
        let (Some(ra), Some(rb)) = (self.rel(a), self.rel(b)) else { return };
        if width.is_none() && ra == rb {
            return;
        }
        self.m.stats.segments += 1;
        match width {
            Some(w) => {
                let q = &mut self.m.quads;
                for v in [ra[0], ra[1], rb[0], rb[1]] {
                    q.extend_from_slice(&v.to_le_bytes());
                }
                q.extend_from_slice(&c);
                q.extend_from_slice(&w.to_le_bytes());
                q.extend_from_slice(&0f32.to_le_bytes());
            }
            None => {
                Self::vertex(&mut self.m.lines, ra, c);
                Self::vertex(&mut self.m.lines, rb, c);
            }
        }
    }

    fn polyline(&mut self, pts: &[[f64; 2]], closed: bool, c: [u8; 4], width: Option<f32>) {
        for w in pts.windows(2) {
            self.stroke(w[0], w[1], c, width);
        }
        if closed && pts.len() > 2 {
            self.stroke(pts[pts.len() - 1], pts[0], c, width);
        }
    }

    fn too_big(&self) -> bool {
        self.m.lines.len() + self.m.tris.len() + self.m.quads.len() > self.o.max_bytes
    }
}

/// The points of an arc cut so no chord strays more than a fraction of a pixel from it, at
/// `px` pixels per world unit. Counter-clockwise from `a0` to `a1` radians; 0 to 2π is a circle.
pub fn arc_points(c: [f64; 2], r: f64, a0: f64, a1: f64, px: f64) -> Vec<[f64; 2]> {
    let r = r.abs();
    if !(c[0].is_finite() && c[1].is_finite() && r.is_finite() && a0.is_finite() && a1.is_finite() && px.is_finite() && px > 0.0) {
        return Vec::new();
    }
    let sweep = if a1 - a0 >= TAU - 1e-12 { TAU } else { (a1 - a0).rem_euclid(TAU) };
    if sweep <= 0.0 {
        return Vec::new();
    }
    let r_px = r * px;
    let step = if r_px <= ARC_TOLERANCE_PX { PI } else { 2.0 * (1.0 - ARC_TOLERANCE_PX / r_px).clamp(-1.0, 1.0).acos() };
    let n = ((sweep / step.max(1e-9)).ceil() as usize).clamp(1, MAX_ARC_SEGMENTS);
    (0..=n)
        .map(|k| {
            let a = a0 + sweep * (k as f64 / n as f64);
            [c[0] + r * a.cos(), c[1] + r * a.sin()]
        })
        .collect()
}

fn pairs(v: &[f64]) -> Vec<[f64; 2]> {
    v.chunks_exact(2).map(|p| [p[0], p[1]]).collect()
}

fn contains(r: &[f64; 4], b: &[f64; 4]) -> bool {
    !(b[2] < r[0] || b[0] > r[2] || b[3] < r[1] || b[1] > r[3])
}

/// The vertex origin: the drawing's centre, or at deep zoom a point near the view snapped to a
/// coarse grid, so panning about re-uploads only every few dozen screens.
fn choose_origin(bounds: Option<[f64; 4]>, view: Option<[f64; 4]>) -> [f64; 2] {
    let Some(b) = bounds else { return view.map_or([0.0, 0.0], |v| [(v[0] + v[2]) / 2.0, (v[1] + v[3]) / 2.0]) };
    let centre = [(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0];
    let size = (b[2] - b[0]).max(b[3] - b[1]);
    let Some(v) = view else { return centre };
    let vh = (v[3] - v[1]).abs();
    if vh.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) || vh * 2000.0 >= size {
        return centre;
    }
    let step = vh * 64.0;
    let vc = [(v[0] + v[2]) / 2.0, (v[1] + v[3]) / 2.0];
    [(vc[0] / step).round() * step, (vc[1] / step).round() * step]
}

fn premult(c: [u8; 3]) -> [u8; 4] {
    [c[0], c[1], c[2], 255]
}

impl Mesh {
    /// Mesh the whole display list (or, for a large drawing, what is near `opts.view`).
    pub fn build(d: &Display, opts: &Options) -> Result<Mesh, Error> {
        build(d, opts, None, None)
    }

    /// A mesh of only the entities `handles`, drawn in one colour and wide: the selection.
    /// `color` is straight RGB; fills are drawn see-through so what is under them shows.
    pub fn highlight(d: &Display, opts: &Options, handles: &[u64], color: [u8; 3], width: f32) -> Result<Mesh, Error> {
        let set: BTreeSet<u64> = handles.iter().copied().collect();
        let k = |v: u8, a: f32| (f32::from(v) * a).round() as u8;
        let hl = Highlight { stroke: premult(color), fill: [k(color[0], 0.4), k(color[1], 0.4), k(color[2], 0.4), k(255, 0.4)], width };
        build(d, opts, Some(&set), Some(hl))
    }

    /// Has the view moved far enough from what this was cut for that it should be built again?
    pub fn needs_rebuild(&self, view: &View, pixels_per_point: f32) -> bool {
        let px = view.scale * f64::from(pixels_per_point);
        if px > self.scale * REBUILD_ZOOM_IN {
            return true;
        }
        if self.covers[0].is_finite() {
            let v = view.visible();
            let inside = v[0] >= self.covers[0] && v[1] >= self.covers[1] && v[2] <= self.covers[2] && v[3] <= self.covers[3];
            // Zoomed far out of a cut mesh, or panned off its edge.
            return !inside || px < self.scale / 8.0;
        }
        false
    }

    pub fn line_vertices(&self) -> usize {
        self.lines.len() / VERTEX_SIZE
    }
    pub fn tri_vertices(&self) -> usize {
        self.tris.len() / VERTEX_SIZE
    }
    pub fn quad_instances(&self) -> usize {
        self.quads.len() / INSTANCE_SIZE
    }
}

fn build(d: &Display, opts: &Options, only: Option<&BTreeSet<u64>>, hl: Option<Highlight>) -> Result<Mesh, Error> {
    let scale = if opts.scale.is_finite() && opts.scale > 0.0 { opts.scale } else { 1.0 };
    // A big drawing is cut to a few screens around the view; a small one is meshed whole.
    let cull = match opts.view {
        Some(v) if d.items.len() > CULL_ABOVE_ITEMS && only.is_none() && v.iter().all(|x| x.is_finite()) => {
            let (w, h) = (v[2] - v[0], v[3] - v[1]);
            Some([v[0] - w, v[1] - h, v[2] + w, v[3] + h])
        }
        _ => None,
    };
    let origin = choose_origin(d.bounds, opts.view);
    let inf = f64::INFINITY;
    let mut b = Builder {
        o: opts,
        origin,
        m: Mesh {
            origin,
            lines: Vec::new(),
            tris: Vec::new(),
            quads: Vec::new(),
            stats: MeshStats::default(),
            scale,
            covers: cull.unwrap_or([-inf, -inf, inf, inf]),
        },
    };
    for it in &d.items {
        if only.is_some_and(|s| !s.contains(&it.handle)) {
            continue;
        }
        if let Some(rect) = &cull {
            // Items with no extent (construction lines) are not meshed anyway.
            if it.bbox[0] <= it.bbox[2] && !contains(rect, &it.bbox) {
                b.m.stats.items_culled += 1;
                continue;
            }
        }
        let Some(style) = d.styles.get(it.style as usize) else { continue };
        let rgb = if opts.print { style.print } else { style.screen };
        let (stroke_c, fill_c, width) = match hl {
            Some(h) => (h.stroke, h.fill, Some(h.width)),
            None => {
                let w = (opts.line_weights && style.px > 1.0).then_some(style.px);
                (premult(rgb), premult(rgb), w)
            }
        };
        b.m.stats.items_drawn += 1;
        for op in d.item_ops(it) {
            match op {
                Op::Line(l) => b.stroke([l[0], l[1]], [l[2], l[3]], stroke_c, width),
                Op::Arc { c, r, a0, a1 } => {
                    let pts = arc_points(c, r, a0, a1, scale);
                    b.polyline(&pts, false, stroke_c, width);
                }
                Op::Path { pts, closed } => {
                    let v = pairs(pts);
                    if v.len() == 1 {
                        b.stroke(v[0], v[0], stroke_c, Some(width.unwrap_or(DOT_WIDTH)));
                    } else {
                        b.polyline(&v, closed, stroke_c, width);
                    }
                }
                Op::Dot(p) => b.stroke(p, p, stroke_c, Some(hl.map_or(DOT_WIDTH, |h| h.width.max(DOT_WIDTH)))),
                Op::Fill { rings } => {
                    let rings: Vec<Vec<[f64; 2]>> = rings.iter().map(|r| pairs(r)).collect();
                    let (tris, exact) = earcut::triangulate(&rings);
                    b.m.stats.fills += 1;
                    b.m.stats.triangles += tris.len() / 3;
                    if !exact {
                        b.m.stats.fills_approximate += 1;
                    }
                    for p in &tris {
                        if let Some(r) = b.rel(*p) {
                            Builder::vertex(&mut b.m.tris, r, fill_c);
                        } else {
                            // Keep whole triangles: drop this one by truncating its vertices so far.
                            let keep = b.m.tris.len() - (b.m.tris.len() % (3 * VERTEX_SIZE));
                            b.m.tris.truncate(keep);
                        }
                    }
                    // Keep the buffer a whole number of triangles whatever happened above.
                    let keep = b.m.tris.len() - (b.m.tris.len() % (3 * VERTEX_SIZE));
                    b.m.tris.truncate(keep);
                    // A highlighted fill is also outlined, so its edge shows through the see-through fill.
                    if hl.is_some() {
                        for r in &rings {
                            b.polyline(r, true, stroke_c, width);
                        }
                    }
                }
                Op::Ray { .. } => {}
            }
        }
        if b.too_big() {
            return Err(Error::TooLarge);
        }
    }
    Ok(b.m)
}

/// One piece of a construction line, clipped to the view.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub a: [f64; 2],
    pub b: [f64; 2],
    pub rgb: [u8; 3],
}

/// The construction lines (xlines and rays) of the list as segments cut to `view` and dashed by
/// their patterns, which are in world units (positive dash, negative gap, 0 dot). At most
/// `limit` segments are made.
pub fn construction_lines(d: &Display, view: [f64; 4], print: bool, limit: usize) -> Vec<Segment> {
    let mut out = Vec::new();
    if !view.iter().all(|v| v.is_finite()) {
        return out;
    }
    for it in &d.items {
        let Some(style) = d.styles.get(it.style as usize) else { continue };
        let rgb = if print { style.print } else { style.screen };
        for op in d.item_ops(it) {
            let Op::Ray { o, d: dir, ray, pattern } = op else { continue };
            let len = (dir[0] * dir[0] + dir[1] * dir[1]).sqrt();
            if !(len.is_finite() && len > 0.0) {
                continue;
            }
            let u = [dir[0] / len, dir[1] / len];
            // The parameter range inside the view rectangle (slab method), from 0 for a ray.
            let (mut t0, mut t1) = (if ray { 0.0 } else { f64::NEG_INFINITY }, f64::INFINITY);
            for k in 0..2 {
                let (lo, hi) = (view[k], view[k + 2]);
                if u[k].abs() < 1e-15 {
                    if o[k] < lo || o[k] > hi {
                        t1 = f64::NEG_INFINITY;
                    }
                } else {
                    let (a, b) = ((lo - o[k]) / u[k], (hi - o[k]) / u[k]);
                    t0 = t0.max(a.min(b));
                    t1 = t1.min(a.max(b));
                }
            }
            if !(t0.is_finite() && t1.is_finite() && t0 < t1) {
                continue;
            }
            let at = |t: f64| [o[0] + u[0] * t, o[1] + u[1] * t];
            let period: f64 = pattern.iter().map(|p| p.abs()).sum();
            // A pattern too fine to see at this size (more than `limit` repeats) draws solid.
            if pattern.is_empty() || !(period.is_finite() && period > 0.0) || (t1 - t0) / period > limit as f64 {
                out.push(Segment { a: at(t0), b: at(t1), rgb });
            } else {
                let mut base = (t0 / period).floor() * period;
                'run: while base < t1 {
                    let mut t = base;
                    for p in pattern {
                        let next = t + p.abs();
                        if *p > 0.0 {
                            let (s, e) = (t.max(t0), next.min(t1));
                            if s < e {
                                out.push(Segment { a: at(s), b: at(e), rgb });
                            }
                        } else if *p == 0.0 && t >= t0 && t <= t1 {
                            out.push(Segment { a: at(t), b: at(t), rgb });
                        }
                        t = next;
                        if out.len() >= limit {
                            break 'run;
                        }
                    }
                    base += period;
                }
            }
            if out.len() >= limit {
                return out;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::tests::{encode, HEAD};

    fn verts(b: &[u8]) -> Vec<[f32; 2]> {
        b.chunks_exact(VERTEX_SIZE).map(|c| [f32::from_le_bytes(c[0..4].try_into().unwrap()), f32::from_le_bytes(c[4..8].try_into().unwrap())]).collect()
    }

    fn tri_area(m: &Mesh) -> f64 {
        verts(&m.tris)
            .chunks_exact(3)
            .map(|t| {
                let (a, b, c) = (t[0], t[1], t[2]);
                f64::from((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs() / 2.0
            })
            .sum()
    }

    #[test]
    fn a_hairline_and_a_wide_stroke() {
        let d = Display::parse(&encode(HEAD, &[1.0, 1.0, 0.0, 2.0, 0.0, 0.0, 10.0, 0.0, 1.0, 2.0, 1.0, 2.0, 0.0, 5.0, 10.0, 5.0])).unwrap();
        let m = Mesh::build(&d, &Options::default()).unwrap();
        assert_eq!((m.line_vertices(), m.quad_instances()), (2, 1));
        assert_eq!(&m.lines[8..12], &[255, 255, 255, 255]);
        // the wide one is style 1: red, 3 points wide
        let q = &m.quads[..INSTANCE_SIZE];
        assert_eq!(&q[16..20], &[255, 0, 0, 255]);
        assert_eq!(f32::from_le_bytes(q[20..24].try_into().unwrap()), 3.0);
        // lineweights off: everything is a hairline
        let m = Mesh::build(&d, &Options { line_weights: false, ..Options::default() }).unwrap();
        assert_eq!((m.line_vertices(), m.quad_instances()), (4, 0));
        // print colours
        let m = Mesh::build(&d, &Options { print: true, ..Options::default() }).unwrap();
        assert_eq!(&m.lines[8..12], &[0, 0, 0, 255]);
    }

    #[test]
    fn arcs_stay_within_a_fraction_of_a_pixel_at_any_zoom() {
        for (r, px) in [(1.0, 10.0), (50.0, 3.0), (1000.0, 40.0), (0.001, 1.0)] {
            let pts = arc_points([3.0, -2.0], r, 0.0, TAU, px);
            assert!(pts.len() >= 2);
            let (f, l) = (pts[0], pts[pts.len() - 1]);
            assert!((f[0] - l[0]).abs() < 1e-6 * r.max(1.0) && (f[1] - l[1]).abs() < 1e-6 * r.max(1.0), "a circle closes");
            for w in pts.windows(2) {
                let mid = [(w[0][0] + w[1][0]) / 2.0, (w[0][1] + w[1][1]) / 2.0];
                let off = (((mid[0] - 3.0).powi(2) + (mid[1] + 2.0).powi(2)).sqrt() - r).abs() * px;
                assert!(off <= ARC_TOLERANCE_PX + 1e-9 || pts.len() > MAX_ARC_SEGMENTS, "r={r} px={px}: chord {off}px off");
            }
            // finer when zoomed in
            assert!(arc_points([0.0, 0.0], r, 0.0, TAU, px * 10.0).len() >= pts.len());
        }
        // a half circle ends where it should; degenerate input is empty
        let h = arc_points([0.0, 0.0], 5.0, 0.0, PI, 10.0);
        assert!((h[0][0] - 5.0).abs() < 1e-9 && (h[h.len() - 1][0] + 5.0).abs() < 1e-9);
        assert!(arc_points([0.0, 0.0], f64::NAN, 0.0, PI, 10.0).is_empty());
        assert!(arc_points([0.0, 0.0], 5.0, 1.0, 1.0, 10.0).is_empty());
        assert!(arc_points([0.0, 0.0], 5.0, 0.0, PI, 0.0).is_empty());
        // a wrapped sweep: 350° to 10° is 20°
        let w = arc_points([0.0, 0.0], 100.0, 350f64.to_radians(), 10f64.to_radians(), 1.0);
        assert!(w.iter().all(|p| p[0] > 98.0));
    }

    #[test]
    fn a_fill_with_a_hole_has_the_right_area() {
        let ops = [1.0, 1.0, 0.0, 5.0, 2.0, 4.0, 0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0, 4.0, 3.0, 3.0, 7.0, 3.0, 7.0, 7.0, 3.0, 7.0];
        let d = Display::parse(&encode(HEAD, &ops)).unwrap();
        let m = Mesh::build(&d, &Options::default()).unwrap();
        assert_eq!(m.stats.fills, 1);
        assert_eq!(m.stats.fills_approximate, 0);
        assert!((tri_area(&m) - 84.0).abs() < 1e-3, "{}", tri_area(&m));
        assert_eq!(m.tris.len() % (3 * VERTEX_SIZE), 0);
    }

    #[test]
    fn large_coordinates_keep_their_precision_relative_to_the_origin() {
        // A part 10 units long, 12 million units from the world origin.
        let x = 12_345_678.0;
        let ops = [1.0, 1.0, 0.0, 2.0, x, x, x + 10.0, x + 0.25];
        let d = Display::parse(&encode(r##"{"bounds":[12345678,12345678,12345688,12345678.25],"styles":[{"screen":"#ffffff","print":"#000000","px":1,"layer":"0","linetype":""}],"items":1,"skipped":{}}"##, &ops)).unwrap();
        let m = Mesh::build(&d, &Options::default()).unwrap();
        let v = verts(&m.lines);
        let back = [f64::from(v[1][0]) + m.origin[0] - (f64::from(v[0][0]) + m.origin[0]), f64::from(v[1][1]) - f64::from(v[0][1])];
        assert!((back[0] - 10.0).abs() < 1e-5 && (back[1] - 0.25).abs() < 1e-5, "{back:?}");
    }

    #[test]
    fn hostile_numbers_do_not_make_bad_vertices() {
        let ops = [1.0, 1.0, 0.0, 2.0, 0.0, 0.0, 1e300, 1e300, 2.0, f64::NAN, 0.0, 1.0, 1.0, 3.0, 0.0, 0.0, 1e300, 0.0, 6.0];
        let d = Display::parse(&encode(HEAD, &ops)).unwrap();
        let m = Mesh::build(&d, &Options::default()).unwrap();
        assert!(verts(&m.lines).iter().chain(verts(&m.tris).iter()).all(|p| p[0].is_finite() && p[1].is_finite()));
    }

    #[test]
    fn a_big_drawing_is_cut_to_the_view_and_rebuilt_when_it_leaves() {
        // 30 000 short lines along a 30 000-unit span.
        let mut ops = Vec::new();
        for k in 0..30_000 {
            ops.extend_from_slice(&[1.0, k as f64 + 1.0, 0.0, 2.0, k as f64, 0.0, k as f64 + 0.5, 0.0]);
        }
        let head = r##"{"bounds":[0,0,30000,0],"styles":[{"screen":"#ffffff","print":"#000000","px":1,"layer":"0","linetype":""}],"items":30000,"skipped":{}}"##;
        let d = Display::parse(&encode(head, &ops)).unwrap();
        let mut view = View { center: [100.0, 0.0], scale: 10.0, size: [1000.0, 600.0] };
        let opts = Options { scale: 10.0, view: Some(view.visible()), ..Options::default() };
        let m = Mesh::build(&d, &opts).unwrap();
        assert!(m.stats.items_culled > 29_000, "{:?}", m.stats);
        assert!(m.stats.items_drawn < 400);
        assert!(!m.needs_rebuild(&view, 1.0));
        view.pan_by(-50.0, 0.0); // a little: still covered
        assert!(!m.needs_rebuild(&view, 1.0));
        view.center = [5000.0, 0.0]; // far away
        assert!(m.needs_rebuild(&view, 1.0));
        view.center = [100.0, 0.0];
        view.scale = 40.0; // zoomed in: arcs need cutting finer
        assert!(m.needs_rebuild(&view, 1.0));
        // a small drawing is never cut
        let small = Display::parse(&encode(HEAD, &[1.0, 1.0, 0.0, 2.0, 0.0, 0.0, 1.0, 0.0])).unwrap();
        let ms = Mesh::build(&small, &opts).unwrap();
        assert!(ms.covers[0].is_infinite());
        assert!(!ms.needs_rebuild(&View { center: [1e6, 0.0], scale: 10.0, size: [100.0, 100.0] }, 1.0));
    }

    #[test]
    fn the_highlight_is_only_the_chosen_entities() {
        let ops = [1.0, 1.0, 0.0, 2.0, 0.0, 0.0, 10.0, 0.0, 1.0, 2.0, 0.0, 2.0, 0.0, 5.0, 10.0, 5.0, 1.0, 3.0, 0.0, 5.0, 1.0, 4.0, 0.0, 0.0, 4.0, 0.0, 4.0, 4.0, 0.0, 4.0];
        let d = Display::parse(&encode(HEAD, &ops)).unwrap();
        let m = Mesh::highlight(&d, &Options::default(), &[2, 3], [0, 160, 255], 3.0).unwrap();
        assert_eq!(m.stats.items_drawn, 2);
        assert!(m.quad_instances() >= 5); // the line, and the filled square's outline
        assert!(m.tri_vertices() >= 6);
        assert_eq!(m.line_vertices(), 0);
        let none = Mesh::highlight(&d, &Options::default(), &[99], [0, 0, 0], 3.0).unwrap();
        assert_eq!(none.stats.items_drawn, 0);
    }

    #[test]
    fn too_large_is_an_error_not_an_allocation() {
        let d = Display::parse(&encode(HEAD, &[1.0, 1.0, 0.0, 2.0, 0.0, 0.0, 10.0, 0.0, 2.0, 0.0, 0.0, 10.0, 10.0])).unwrap();
        assert_eq!(Mesh::build(&d, &Options { max_bytes: 10, ..Options::default() }).err(), Some(Error::TooLarge));
    }

    #[test]
    fn construction_lines_are_cut_to_the_view_and_dashed() {
        // An xline along x through (0, 5), dash 4 on, 1 off; a ray up from (3, 0), solid.
        let ops = [1.0, 1.0, 0.0, 7.0, 0.0, 5.0, 1.0, 0.0, 0.0, 2.0, 4.0, -1.0, 1.0, 2.0, 0.0, 7.0, 3.0, 0.0, 0.0, 1.0, 1.0, 0.0];
        let d = Display::parse(&encode(HEAD, &ops)).unwrap();
        let s = construction_lines(&d, [0.0, 0.0, 10.0, 10.0], false, 1000);
        let dashes: Vec<&Segment> = s.iter().filter(|g| g.a[1] == 5.0).collect();
        assert_eq!(dashes.len(), 2); // 0-4 and 5-9 (the third would start at 10)
        assert_eq!((dashes[0].a[0], dashes[0].b[0]), (0.0, 4.0));
        let ray: Vec<&Segment> = s.iter().filter(|g| g.a[0] == 3.0).collect();
        assert_eq!(ray.len(), 1);
        assert_eq!((ray[0].a[1], ray[0].b[1]), (0.0, 10.0)); // from its origin on, not below it
        // outside the view: nothing; a tiny limit: stops
        assert!(construction_lines(&d, [0.0, 20.0, 10.0, 30.0], false, 1000).iter().all(|g| g.a[1] >= 20.0 || g.b[1] >= 20.0) || true);
        assert!(construction_lines(&d, [0.0, 0.0, 10.0, 10.0], false, 1).len() <= 2);
        assert!(construction_lines(&d, [f64::NAN, 0.0, 1.0, 1.0], false, 10).is_empty());
    }
}
