//! The display list as shapes in screen coordinates: what an app paints.
//!
//! Everything a canvas needs that is not the toolkit's own is done here:
//! clipping to the viewport (so a line from far away stays straight), arcs
//! flattened to the tolerance the zoom needs, construction lines cut to the
//! view and dashed, and even-odd fills cut into triangles. An item that is
//! off screen costs one rectangle test.
//!
//! The clipping and the line-count guards follow CADCraft's canvas drawing
//! (crates/ui-egui/src/canvas/draw.rs, MIT OR Apache-2.0; see NOTICE).

use crate::display::{Bounds, Display, Op, Rgb};
use crate::view::View;
use std::f64::consts::{PI, TAU};

/// Segments of an arc, at most; a bigger arc is drawn coarser.
pub const MAX_ARC_SEGMENTS: usize = 4096;
/// Dashes of one construction line, at most; more and it is drawn solid.
pub const MAX_DASHES: f64 = 20_000.0;
/// Edges of one fill, and band-edge tests over all its bands, at most;
/// more and the fill is not drawn (its outline still is).
pub const MAX_FILL_EDGES: usize = 20_000;
pub const MAX_FILL_WORK: usize = 8_000_000;
/// Points past the viewport edge that are still drawn, so a line's end cap
/// is not cut at the edge.
const MARGIN: f64 = 4.0;

#[derive(Debug, Clone)]
pub struct DrawOptions {
    /// Print colours on white paper, not screen colours on the dark canvas.
    pub paper: bool,
    /// How far, in points, a flattened arc may stray from the true one.
    pub arc_tolerance: f64,
    /// Layers not drawn.
    pub hidden_layers: Vec<String>,
}

impl Default for DrawOptions {
    fn default() -> Self {
        DrawOptions {
            paper: false,
            arc_tolerance: 0.25,
            hidden_layers: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    /// A polyline (two points or more), closed back to its start if `closed`.
    Line {
        points: Vec<[f32; 2]>,
        closed: bool,
    },
    /// Filled triangles, three points each.
    Fill {
        triangles: Vec<[f32; 2]>,
    },
    Dot([f32; 2]),
}

/// A shape with its colour and width, and the item it came from (an index
/// into [`Display::items`], to find the entity under a click or hover).
#[derive(Debug, Clone, PartialEq)]
pub struct Prim {
    pub item: usize,
    pub color: Rgb,
    /// Line width in points.
    pub width: f32,
    pub shape: Shape,
}

/// Every visible item of `d` as shapes for `view`.
pub fn tessellate(d: &Display, view: &View, opts: &DrawOptions) -> Vec<Prim> {
    let mut out = Vec::new();
    for i in 0..d.items.len() {
        tessellate_item(d, i, view, opts, &mut out);
    }
    out
}

/// The shapes of item `index`, appended to `out`; nothing if it is hidden
/// or off screen. Call it alone to draw one item again in another colour
/// (a hover or a selection).
pub fn tessellate_item(
    d: &Display,
    index: usize,
    view: &View,
    opts: &DrawOptions,
    out: &mut Vec<Prim>,
) {
    let Some(item) = d.items.get(index) else {
        return;
    };
    let Some(style) = d.styles.get(item.style) else {
        return;
    };
    if opts.hidden_layers.contains(&style.layer) {
        return;
    }
    let near = MARGIN / view.scale;
    let visible = view.world_bounds().grown(near);
    if !item.bounds.intersects(&visible) {
        return;
    }
    let color = if opts.paper {
        style.print
    } else {
        style.screen
    };
    let width = style.px as f32;
    let mut put = |shape: Shape| {
        out.push(Prim {
            item: index,
            color,
            width,
            shape,
        })
    };
    for op in &item.ops {
        if !op.bounds().intersects(&visible) {
            continue;
        }
        match op {
            Op::Line(l) => lines(view, &[[l[0], l[1]], [l[2], l[3]]], false, &mut put),
            Op::Arc {
                center,
                radius,
                start,
                end,
            } => match arc_points(view, *center, *radius, *start, *end, opts.arc_tolerance) {
                Arc::Dot(p) => put_dot(view, p, &mut put),
                Arc::Points(p) => runs(view, &p, false, &mut put),
                Arc::None => {}
            },
            Op::Path { closed, points } => {
                let screen: Vec<[f64; 2]> = points.iter().map(|p| view.to_screen_f64(*p)).collect();
                runs(view, &screen, *closed, &mut put);
            }
            Op::Fill(rings) => {
                let screen: Vec<Vec<[f64; 2]>> = rings
                    .iter()
                    .map(|r| r.iter().map(|p| view.to_screen_f64(*p)).collect())
                    .collect();
                match fill_triangles(&screen, view.size) {
                    Some(triangles) if !triangles.is_empty() => put(Shape::Fill { triangles }),
                    // Too big to fill: the outline still shows where it is.
                    None => screen.iter().for_each(|r| runs(view, r, true, &mut put)),
                    Some(_) => {}
                }
            }
            Op::Dot(p) => put_dot(view, view.to_screen_f64(*p), &mut put),
            Op::Ray {
                origin,
                dir,
                ray,
                dashes,
            } => ray_shapes(view, *origin, *dir, *ray, dashes, &mut put),
        }
    }
}

fn lines(view: &View, world: &[[f64; 2]], closed: bool, put: &mut impl FnMut(Shape)) {
    let screen: Vec<[f64; 2]> = world.iter().map(|p| view.to_screen_f64(*p)).collect();
    runs(view, &screen, closed, put);
}

fn put_dot(view: &View, s: [f64; 2], put: &mut impl FnMut(Shape)) {
    let [w, h] = view.size;
    if s[0] >= -MARGIN
        && s[0] <= f64::from(w) + MARGIN
        && s[1] >= -MARGIN
        && s[1] <= f64::from(h) + MARGIN
    {
        put(Shape::Dot([s[0] as f32, s[1] as f32]));
    }
}

/// The part of the segment `a`–`b` inside the viewport (and its margin), by
/// Liang–Barsky, in f64 so far-away ends do not bend the line.
pub fn clip_segment(a: [f64; 2], b: [f64; 2], size: [f32; 2]) -> Option<([f64; 2], [f64; 2])> {
    let (lo, hi) = (
        [-MARGIN, -MARGIN],
        [f64::from(size[0]) + MARGIN, f64::from(size[1]) + MARGIN],
    );
    let d = [b[0] - a[0], b[1] - a[1]];
    if !(a[0].is_finite() && a[1].is_finite() && d[0].is_finite() && d[1].is_finite()) {
        return None;
    }
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for k in 0..2 {
        if d[k] == 0.0 {
            if a[k] < lo[k] || a[k] > hi[k] {
                return None;
            }
        } else {
            let (mut u0, mut u1) = ((lo[k] - a[k]) / d[k], (hi[k] - a[k]) / d[k]);
            if u0 > u1 {
                std::mem::swap(&mut u0, &mut u1);
            }
            t0 = t0.max(u0);
            t1 = t1.min(u1);
            if t0 > t1 {
                return None;
            }
        }
    }
    Some((
        [a[0] + d[0] * t0, a[1] + d[1] * t0],
        [a[0] + d[0] * t1, a[1] + d[1] * t1],
    ))
}

/// A polyline in screen coordinates as the runs of it that are inside the
/// viewport.
fn runs(view: &View, pts: &[[f64; 2]], closed: bool, put: &mut impl FnMut(Shape)) {
    if pts.len() < 2 {
        return;
    }
    let n = pts.len() + usize::from(closed);
    let mut run: Vec<[f32; 2]> = Vec::new();
    let flush = |run: &mut Vec<[f32; 2]>, put: &mut dyn FnMut(Shape)| {
        if run.len() >= 2 {
            put(Shape::Line {
                points: std::mem::take(run),
                closed: false,
            });
        } else {
            run.clear();
        }
    };
    for k in 1..n {
        let (Some(&p), Some(&q)) = (pts.get(k - 1), pts.get(k % pts.len())) else {
            break;
        };
        match clip_segment(p, q, view.size) {
            Some((a, b)) => {
                let (a, b) = ([a[0] as f32, a[1] as f32], [b[0] as f32, b[1] as f32]);
                if run.last() != Some(&a) {
                    flush(&mut run, put);
                    run.push(a);
                }
                run.push(b);
            }
            None => flush(&mut run, put),
        }
    }
    flush(&mut run, put);
}

enum Arc {
    None,
    /// Too small to see as a curve: a point, in screen coordinates.
    Dot([f64; 2]),
    /// The flattened arc, in screen coordinates.
    Points(Vec<[f64; 2]>),
}

fn arc_points(
    view: &View,
    center: [f64; 2],
    radius: f64,
    start: f64,
    end: f64,
    tolerance: f64,
) -> Arc {
    let r_px = radius * view.scale;
    let (turn, c) = (end - start, view.to_screen_f64(center));
    if !(r_px.is_finite()
        && turn.is_finite()
        && start.is_finite()
        && center[0].is_finite()
        && center[1].is_finite())
        || turn == 0.0
    {
        return Arc::None;
    }
    if r_px < 0.05 {
        return Arc::Dot(c);
    }
    // Counter-clockwise; a full circle is 0 to 2π.
    let sweep = if turn.abs() >= TAU {
        TAU
    } else {
        turn.rem_euclid(TAU)
    };
    let tol = if tolerance.is_finite() && tolerance > 0.0 {
        tolerance
    } else {
        0.25
    };
    let step = if r_px > tol {
        2.0 * (1.0 - tol / r_px).acos()
    } else {
        PI / 2.0
    };
    let n = if step.is_finite() && step > 0.0 {
        (sweep / step).ceil().clamp(1.0, MAX_ARC_SEGMENTS as f64) as usize
    } else {
        MAX_ARC_SEGMENTS
    };
    let pts = (0..=n)
        .map(|k| {
            let a = start + sweep * (k as f64 / n as f64);
            // y points up in the world, down on the screen
            [c[0] + r_px * a.cos(), c[1] - r_px * a.sin()]
        })
        .collect();
    Arc::Points(pts)
}

/// A construction line, cut to the viewport and dashed.
fn ray_shapes(
    view: &View,
    origin: [f64; 2],
    dir: [f64; 2],
    ray: bool,
    dashes: &[f64],
    put: &mut impl FnMut(Shape),
) {
    let len = dir[0].hypot(dir[1]);
    if !(len.is_finite() && len > 1e-12 && origin[0].is_finite() && origin[1].is_finite()) {
        return;
    }
    let (ux, uy) = (dir[0] / len, dir[1] / len);
    let o = view.to_screen_f64(origin);
    // Screen movement per world unit along the line.
    let d = [ux * view.scale, -uy * view.scale];
    let (lo, hi) = (
        [-MARGIN, -MARGIN],
        [
            f64::from(view.size[0]) + MARGIN,
            f64::from(view.size[1]) + MARGIN,
        ],
    );
    let (mut ta, mut tb) = (if ray { 0.0 } else { f64::NEG_INFINITY }, f64::INFINITY);
    for k in 0..2 {
        if d[k].abs() < 1e-300 {
            if o[k] < lo[k] || o[k] > hi[k] {
                return;
            }
        } else {
            let (mut u0, mut u1) = ((lo[k] - o[k]) / d[k], (hi[k] - o[k]) / d[k]);
            if u0 > u1 {
                std::mem::swap(&mut u0, &mut u1);
            }
            ta = ta.max(u0);
            tb = tb.min(u1);
        }
    }
    if !(ta.is_finite() && tb.is_finite()) || ta > tb {
        return;
    }
    let at = |t: f64| [(o[0] + d[0] * t) as f32, (o[1] + d[1] * t) as f32];
    let period: f64 = dashes.iter().map(|x| x.abs()).sum();
    let solid = |put: &mut dyn FnMut(Shape)| {
        put(Shape::Line {
            points: vec![at(ta), at(tb)],
            closed: false,
        })
    };
    // How many dashes the visible length holds.
    let count = if period > 0.0 {
        (tb - ta) / period * dashes.len() as f64
    } else {
        0.0
    };
    if dashes.is_empty()
        || period <= 0.0
        || period.is_nan()
        || !period.is_finite()
        || !count.is_finite()
        || count > MAX_DASHES
        || period * view.scale < 0.25
    {
        solid(put);
        return;
    }
    let mut t = (ta / period).floor() * period;
    while t < tb {
        for &s in dashes {
            let seg = s.abs();
            if s > 0.0 {
                let (a, b) = (t.max(ta), (t + seg).min(tb));
                if a < b {
                    put(Shape::Line {
                        points: vec![at(a), at(b)],
                        closed: false,
                    });
                }
            } else if s == 0.0 && t >= ta && t <= tb {
                put(Shape::Dot(at(t)));
            }
            t += seg;
            if t >= tb {
                break;
            }
        }
    }
}

/// Even-odd fill of `rings` (screen coordinates) as triangles, for the part
/// inside the viewport. The rings are cut into horizontal bands at every
/// vertex height; in each band the edges that cross it, taken in pairs from
/// the left, bound trapezoids. Edges that cross each other inside a band
/// (a self-crossing outline) are drawn slightly off in that band. `None`
/// when the fill is too large to cut.
pub fn fill_triangles(rings: &[Vec<[f64; 2]>], size: [f32; 2]) -> Option<Vec<[f32; 2]>> {
    // Edges with y0 < y1; level ones bound no band.
    let mut edges: Vec<[f64; 4]> = Vec::new();
    for ring in rings {
        let n = ring.len();
        for k in 0..n {
            let (Some(p), Some(q)) = (ring.get(k), ring.get((k + 1) % n)) else {
                continue;
            };
            if !(p[0].is_finite() && p[1].is_finite() && q[0].is_finite() && q[1].is_finite()) {
                return None;
            }
            if p[1] < q[1] {
                edges.push([p[0], p[1], q[0], q[1]]);
            } else if p[1] > q[1] {
                edges.push([q[0], q[1], p[0], p[1]]);
            }
            if edges.len() > MAX_FILL_EDGES {
                return None;
            }
        }
    }
    let (top, bottom) = (-MARGIN, f64::from(size[1]) + MARGIN);
    let mut ys = vec![top, bottom];
    ys.extend(
        edges
            .iter()
            .flat_map(|e| [e[1], e[3]])
            .filter(|y| *y > top && *y < bottom),
    );
    ys.sort_by(|a, b| a.total_cmp(b));
    ys.dedup();
    if ys.len().saturating_mul(edges.len()) > MAX_FILL_WORK {
        return None;
    }
    let x_at = |e: &[f64; 4], y: f64| e[0] + (e[2] - e[0]) * (y - e[1]) / (e[3] - e[1]);
    let clampx = |x: f64| x.clamp(-1.0e6, 1.0e6) as f32;
    let mut tris: Vec<[f32; 2]> = Vec::new();
    for band in ys.windows(2) {
        let (ya, yb) = (band[0], band[1]);
        if yb - ya < 1e-9 {
            continue;
        }
        let mid = (ya + yb) / 2.0;
        let mut cross: Vec<&[f64; 4]> = edges.iter().filter(|e| e[1] < mid && mid < e[3]).collect();
        cross.sort_by(|a, b| x_at(a, mid).total_cmp(&x_at(b, mid)));
        for pair in cross.chunks_exact(2) {
            let (l, r) = (pair[0], pair[1]);
            let (la, lb, ra, rb) = (
                clampx(x_at(l, ya)),
                clampx(x_at(l, yb)),
                clampx(x_at(r, ya)),
                clampx(x_at(r, yb)),
            );
            let (fa, fb) = (ya as f32, yb as f32);
            tris.extend_from_slice(&[[la, fa], [ra, fa], [rb, fb], [la, fa], [rb, fb], [lb, fb]]);
        }
        if tris.len() > 3_000_000 {
            return None;
        }
    }
    Some(tris)
}

/// The extent of a shape's points, for tests and hit boxes.
pub fn shape_bounds(shape: &Shape) -> Bounds {
    let mut b = Bounds::EMPTY;
    let mut add = |p: &[f32; 2]| b.include([f64::from(p[0]), f64::from(p[1])]);
    match shape {
        Shape::Line { points, .. } => points.iter().for_each(&mut add),
        Shape::Fill { triangles } => triangles.iter().for_each(&mut add),
        Shape::Dot(p) => add(p),
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::Style;

    fn display(ops: Vec<Op>) -> Display {
        let style = Style {
            screen: Rgb(1, 2, 3),
            print: Rgb(4, 5, 6),
            px: 1.5,
            layer: "L".into(),
            linetype: "CONTINUOUS".into(),
        };
        let mut bounds = Bounds::EMPTY;
        ops.iter().for_each(|o| bounds.union(&o.bounds()));
        Display {
            bounds: None,
            styles: vec![style],
            items: vec![crate::display::Item {
                handle: 7,
                style: 0,
                ops,
                bounds,
            }],
            skipped: vec![],
        }
    }

    fn view() -> View {
        let mut v = View::new([200.0, 100.0]);
        v.center = [100.0, 50.0];
        v.scale = 1.0;
        v
    }

    fn area(tris: &[[f32; 2]]) -> f64 {
        tris.chunks_exact(3)
            .map(|t| {
                let (a, b, c) = ((t[0][0], t[0][1]), (t[1][0], t[1][1]), (t[2][0], t[2][1]));
                f64::from(((b.0 - a.0) * (c.1 - a.1) - (c.0 - a.0) * (b.1 - a.1)).abs()) / 2.0
            })
            .sum()
    }

    #[test]
    fn clipping_is_exact_and_keeps_the_slope() {
        let size = [100.0, 100.0];
        // inside: untouched
        assert_eq!(
            clip_segment([10.0, 10.0], [90.0, 20.0], size),
            Some(([10.0, 10.0], [90.0, 20.0]))
        );
        // from far away on the diagonal: the line stays on the diagonal
        let (a, b) = clip_segment([-1.0e9, -1.0e9], [50.0, 50.0], size).unwrap();
        assert!((a[0] - a[1]).abs() < 1e-3 && (b[0] - 50.0).abs() < 1e-6 && a[0] >= -4.0 - 1e-6);
        // outside entirely, level and not
        assert_eq!(clip_segment([-10.0, -10.0], [-10.0, 500.0], size), None);
        assert_eq!(clip_segment([0.0, 200.0], [100.0, 200.0], size), None);
        assert_eq!(clip_segment([f64::NAN, 0.0], [1.0, 1.0], size), None);
        assert_eq!(clip_segment([0.0, 0.0], [f64::INFINITY, 1.0], size), None);
    }

    #[test]
    fn a_line_inside_is_one_shape_with_the_style() {
        let d = display(vec![Op::Line([10.0, 10.0, 60.0, 10.0])]);
        let prims = tessellate(&d, &view(), &DrawOptions::default());
        assert_eq!(prims.len(), 1);
        let p = &prims[0];
        assert_eq!((p.item, p.color, p.width), (0, Rgb(1, 2, 3), 1.5));
        // world (10,10) with the view centred on (100,50) at 200×100: x 10, y 90
        assert_eq!(
            p.shape,
            Shape::Line {
                points: vec![[10.0, 90.0], [60.0, 90.0]],
                closed: false
            }
        );
        let paper = tessellate(
            &d,
            &view(),
            &DrawOptions {
                paper: true,
                ..Default::default()
            },
        );
        assert_eq!(paper[0].color, Rgb(4, 5, 6));
    }

    #[test]
    fn off_screen_and_hidden_items_make_nothing() {
        let d = display(vec![Op::Line([1000.0, 1000.0, 1100.0, 1000.0])]);
        assert!(tessellate(&d, &view(), &DrawOptions::default()).is_empty());
        let on = display(vec![Op::Line([10.0, 10.0, 60.0, 10.0])]);
        let hide = DrawOptions {
            hidden_layers: vec!["L".into()],
            ..Default::default()
        };
        assert!(tessellate(&on, &view(), &hide).is_empty());
        assert!(
            tessellate(
                &on,
                &view(),
                &DrawOptions {
                    hidden_layers: vec!["OTHER".into()],
                    ..Default::default()
                }
            )
            .len()
                == 1
        );
        let mut out = Vec::new();
        tessellate_item(&on, 99, &view(), &DrawOptions::default(), &mut out);
        assert!(out.is_empty(), "no such item");
    }

    #[test]
    fn a_long_line_is_cut_at_the_edge_not_bent() {
        let d = display(vec![Op::Line([-1.0e7, 50.0, 1.0e7, 50.0])]);
        let p = tessellate(&d, &view(), &DrawOptions::default());
        let Shape::Line { points, .. } = &p[0].shape else {
            panic!()
        };
        assert!(
            points[0][0] >= -4.5
                && points[1][0] <= 204.5
                && points[0][1] == 50.0
                && points[1][1] == 50.0
        );
    }

    #[test]
    fn paths_run_through_the_edge_in_pieces_and_close() {
        let tri = Op::Path {
            closed: true,
            points: vec![[20.0, 20.0], [80.0, 20.0], [50.0, 70.0]],
        };
        let p = tessellate(&display(vec![tri]), &view(), &DrawOptions::default());
        assert_eq!(p.len(), 1);
        let Shape::Line { points, .. } = &p[0].shape else {
            panic!()
        };
        assert_eq!(points.len(), 4, "three corners and back to the first");
        assert_eq!(points[0], points[3]);
        // a path that leaves the view and comes back is two runs
        let out_and_in = Op::Path {
            closed: false,
            points: vec![
                [10.0, 10.0],
                [10.0, 90.0],
                [-500.0, 90.0],
                [-500.0, 10.0],
                [20.0, 10.0],
            ],
        };
        let q = tessellate(&display(vec![out_and_in]), &view(), &DrawOptions::default());
        assert!(q.len() >= 2, "{} runs", q.len());
    }

    #[test]
    fn arcs_stay_within_tolerance_and_end_where_they_should() {
        let mut v = view();
        v.scale = 4.0;
        let arc = Op::Arc {
            center: [100.0, 50.0],
            radius: 10.0,
            start: 0.0,
            end: PI / 2.0,
        };
        let p = tessellate(&display(vec![arc]), &v, &DrawOptions::default());
        let Shape::Line { points, .. } = &p[0].shape else {
            panic!()
        };
        assert!(points.len() > 6);
        let c = v.to_screen([100.0, 50.0]);
        for q in points {
            let r = f64::from((q[0] - c[0]).hypot(q[1] - c[1]));
            assert!((r - 40.0).abs() < 0.3, "radius {r}");
        }
        // starts to the right of the centre, ends above it (y up in the world)
        assert!((points[0][0] - (c[0] + 40.0)).abs() < 1e-3 && (points[0][1] - c[1]).abs() < 1e-3);
        let last = points[points.len() - 1];
        assert!((last[0] - c[0]).abs() < 1e-3 && (last[1] - (c[1] - 40.0)).abs() < 1e-3);
    }

    #[test]
    fn a_circle_closes_and_a_huge_arc_stays_bounded() {
        let circle = Op::Arc {
            center: [100.0, 50.0],
            radius: 30.0,
            start: 0.0,
            end: TAU,
        };
        let p = tessellate(&display(vec![circle]), &view(), &DrawOptions::default());
        let Shape::Line { points, .. } = &p[0].shape else {
            panic!()
        };
        let (a, b) = (points[0], points[points.len() - 1]);
        assert!((a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3);
        let mut v = view();
        v.scale = 1.0e8;
        let big = Op::Arc {
            center: [100.0, 50.0],
            radius: 1.0,
            start: 0.0,
            end: TAU,
        };
        let q = tessellate(&display(vec![big]), &v, &DrawOptions::default());
        let total: usize = q
            .iter()
            .map(|p| {
                if let Shape::Line { points, .. } = &p.shape {
                    points.len()
                } else {
                    0
                }
            })
            .sum();
        assert!(total <= MAX_ARC_SEGMENTS + 1);
        // a speck of a circle is a dot, not a curve; a zero arc is nothing
        let mut tiny = view();
        tiny.scale = 0.001;
        let dot = tessellate(
            &display(vec![Op::Arc {
                center: [100.0, 50.0],
                radius: 1.0,
                start: 0.0,
                end: TAU,
            }]),
            &tiny,
            &DrawOptions::default(),
        );
        assert!(matches!(dot[0].shape, Shape::Dot(_)));
        assert!(
            tessellate(
                &display(vec![Op::Arc {
                    center: [100.0, 50.0],
                    radius: 5.0,
                    start: 1.0,
                    end: 1.0
                }]),
                &view(),
                &DrawOptions::default()
            )
            .is_empty()
        );
    }

    #[test]
    fn a_fill_with_a_hole_has_the_right_area() {
        let outer = vec![[20.0, 20.0], [120.0, 20.0], [120.0, 80.0], [20.0, 80.0]];
        let hole = vec![[50.0, 40.0], [80.0, 40.0], [80.0, 60.0], [50.0, 60.0]];
        let t = fill_triangles(&[outer.clone(), hole], [200.0, 100.0]).unwrap();
        assert!(
            (area(&t) - (100.0 * 60.0 - 30.0 * 20.0)).abs() < 1e-2,
            "{}",
            area(&t)
        );
        // an L shape (concave)
        let l = vec![
            [10.0, 10.0],
            [60.0, 10.0],
            [60.0, 30.0],
            [30.0, 30.0],
            [30.0, 70.0],
            [10.0, 70.0],
        ];
        let t = fill_triangles(&[l], [200.0, 100.0]).unwrap();
        assert!(
            (area(&t) - (50.0 * 20.0 + 20.0 * 40.0)).abs() < 1e-2,
            "{}",
            area(&t)
        );
        // a ring bigger than the view is cut at the top and bottom of the view
        // (and across to 1e6 sideways, which the painter's clip rectangle cuts)
        let huge = vec![
            [-1.0e6, -1.0e6],
            [1.0e6, -1.0e6],
            [1.0e6, 1.0e6],
            [-1.0e6, 1.0e6],
        ];
        let t = fill_triangles(&[huge], [200.0, 100.0]).unwrap();
        assert!((area(&t) - (2.0e6 * 108.0)).abs() < 1.0, "{}", area(&t));
        assert!(
            t.iter()
                .all(|p| p[1] >= -4.0 && p[1] <= 104.0 && p[0].abs() <= 1.0e6)
        );
        assert_eq!(
            fill_triangles(
                &[vec![[f64::NAN, 0.0], [1.0, 1.0], [2.0, 0.0]]],
                [10.0, 10.0]
            ),
            None
        );
        assert!(fill_triangles(&[], [10.0, 10.0]).unwrap().is_empty());
    }

    #[test]
    fn too_big_a_fill_falls_back_to_its_outline() {
        let ring: Vec<[f64; 2]> = (0..=MAX_FILL_EDGES)
            .map(|i| [(i % 200) as f64, (i / 200) as f64 * 0.001 + (i % 2) as f64])
            .collect();
        assert_eq!(
            fill_triangles(std::slice::from_ref(&ring), [200.0, 100.0]),
            None
        );
        let p = tessellate(
            &display(vec![Op::Fill(vec![ring])]),
            &view(),
            &DrawOptions::default(),
        );
        assert!(!p.is_empty() && p.iter().all(|p| matches!(p.shape, Shape::Line { .. })));
    }

    fn lines_of(p: &[Prim]) -> Vec<(Bounds, Vec<[f32; 2]>)> {
        p.iter()
            .filter_map(|p| {
                if let Shape::Line { points, .. } = &p.shape {
                    Some((shape_bounds(&p.shape), points.clone()))
                } else {
                    None
                }
            })
            .collect()
    }

    #[test]
    fn construction_lines_are_cut_to_the_view() {
        let both = Op::Ray {
            origin: [100.0, 50.0],
            dir: [1.0, 0.0],
            ray: false,
            dashes: vec![],
        };
        let p = tessellate(&display(vec![both]), &view(), &DrawOptions::default());
        let l = lines_of(&p);
        assert_eq!(l.len(), 1);
        assert!(
            l[0].0.min[0] <= -3.9 && l[0].0.max[0] >= 203.9,
            "{:?}",
            l[0].0
        );
        // one way: nothing behind the origin
        let one = Op::Ray {
            origin: [100.0, 50.0],
            dir: [1.0, 0.0],
            ray: true,
            dashes: vec![],
        };
        let l = lines_of(&tessellate(
            &display(vec![one]),
            &view(),
            &DrawOptions::default(),
        ));
        assert!((l[0].0.min[0] - 100.0).abs() < 1e-3 && l[0].0.max[0] >= 203.9);
        // a ray pointing away from the view
        let away = Op::Ray {
            origin: [500.0, 50.0],
            dir: [1.0, 0.0],
            ray: true,
            dashes: vec![],
        };
        assert!(tessellate(&display(vec![away]), &view(), &DrawOptions::default()).is_empty());
        // a diagonal through the view, and a zero direction
        let diag = Op::Ray {
            origin: [100.0, 50.0],
            dir: [3.0, 4.0],
            ray: false,
            dashes: vec![],
        };
        assert_eq!(
            lines_of(&tessellate(
                &display(vec![diag]),
                &view(),
                &DrawOptions::default()
            ))
            .len(),
            1
        );
        let zero = Op::Ray {
            origin: [100.0, 50.0],
            dir: [0.0, 0.0],
            ray: false,
            dashes: vec![],
        };
        assert!(tessellate(&display(vec![zero]), &view(), &DrawOptions::default()).is_empty());
    }

    #[test]
    fn dashed_construction_lines_follow_the_pattern_from_the_origin() {
        // 10 on, 10 off, from the origin to the right: dashes at 0..10, 20..30, ...
        let r = Op::Ray {
            origin: [100.0, 50.0],
            dir: [1.0, 0.0],
            ray: true,
            dashes: vec![10.0, -10.0],
        };
        let l = lines_of(&tessellate(
            &display(vec![r]),
            &view(),
            &DrawOptions::default(),
        ));
        assert!(l.len() >= 5, "{} dashes", l.len());
        assert!((l[0].1[0][0] - 100.0).abs() < 1e-3 && (l[0].1[1][0] - 110.0).abs() < 1e-3);
        assert!((l[1].1[0][0] - 120.0).abs() < 1e-3);
        // a dot (0) draws a point
        let dotted = Op::Ray {
            origin: [100.0, 50.0],
            dir: [1.0, 0.0],
            ray: true,
            dashes: vec![0.0, -20.0],
        };
        let p = tessellate(&display(vec![dotted]), &view(), &DrawOptions::default());
        assert!(p.len() >= 4 && p.iter().all(|p| matches!(p.shape, Shape::Dot(_))));
        // the same pattern both ways is anchored at the origin on both sides
        let both = Op::Ray {
            origin: [100.0, 50.0],
            dir: [1.0, 0.0],
            ray: false,
            dashes: vec![10.0, -10.0],
        };
        let l = lines_of(&tessellate(
            &display(vec![both]),
            &view(),
            &DrawOptions::default(),
        ));
        assert!(l.iter().any(|(_, pts)| (pts[0][0] - 100.0).abs() < 1e-3));
        assert!(
            l.iter().any(|(b, _)| b.max[0] < 100.0),
            "dashes behind the origin too"
        );
    }

    #[test]
    fn a_pattern_too_fine_to_see_is_a_solid_line() {
        let r = Op::Ray {
            origin: [100.0, 50.0],
            dir: [1.0, 0.0],
            ray: false,
            dashes: vec![1.0e-5, -1.0e-5],
        };
        let p = tessellate(&display(vec![r]), &view(), &DrawOptions::default());
        assert_eq!(p.len(), 1);
        let many = Op::Ray {
            origin: [100.0, 50.0],
            dir: [1.0, 0.0],
            ray: false,
            dashes: vec![0.01, -0.01],
        };
        let mut v = view();
        v.scale = 1.0e4;
        let q = tessellate(&display(vec![many]), &v, &DrawOptions::default());
        assert!(q.len() <= 2 * (MAX_DASHES as usize), "{}", q.len());
    }

    #[test]
    fn dots_show_only_in_view() {
        let near = tessellate(
            &display(vec![Op::Dot([100.0, 50.0])]),
            &view(),
            &DrawOptions::default(),
        );
        assert_eq!(near[0].shape, Shape::Dot([100.0, 50.0]));
        assert!(
            tessellate(
                &display(vec![Op::Dot([1.0e5, 50.0])]),
                &view(),
                &DrawOptions::default()
            )
            .is_empty()
        );
    }

    #[test]
    fn a_real_drawing_tessellates_to_finite_shapes() {
        let mut s = cad_ffi::session::Session::new();
        let id = s.sample().unwrap();
        let d = Display::parse(&s.display(id).unwrap()).unwrap();
        assert!(!d.items.is_empty());
        let mut v = View::new([1000.0, 700.0]);
        v.zoom_extents(&d.extents().unwrap(), 20.0);
        let prims = tessellate(&d, &v, &DrawOptions::default());
        assert!(
            prims.len() >= d.items.len() / 2,
            "{} shapes from {} items",
            prims.len(),
            d.items.len()
        );
        for p in &prims {
            let b = shape_bounds(&p.shape);
            assert!(
                b.is_finite() && b.min[0] >= -1.0e6 && b.max[0] <= 1.0e6,
                "{:?}",
                p.shape
            );
            assert!(p.item < d.items.len());
        }
        // zoomed in on a corner, far less is drawn and nothing is out of range
        let mut z = v;
        z.zoom_about(500.0, [10.0, 10.0]);
        let few = tessellate(&d, &z, &DrawOptions::default());
        assert!(few.len() < prims.len());
        assert!(few.iter().all(|p| shape_bounds(&p.shape).is_finite()));
    }
}
