//! The display list (`cad-core`'s `export::display`, layout in its module
//! documentation) decoded into items an app can draw: lines, true arcs,
//! polylines, fills, dots and construction lines, in world coordinates,
//! with the style table.
//!
//! The bytes are data like any other file: every count is checked against
//! the bytes left before anything is allocated, and nothing panics.

use crate::json::Json;

pub const MAGIC: &[u8; 4] = b"DSPL";
pub const LAYOUT_VERSION: u32 = 1;

/// A rectangle in world coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub min: [f64; 2],
    pub max: [f64; 2],
}

impl Bounds {
    /// Contains nothing; the first `include` sets it.
    pub const EMPTY: Bounds = Bounds {
        min: [f64::INFINITY; 2],
        max: [f64::NEG_INFINITY; 2],
    };
    /// Contains everything (a construction line has no ends).
    pub const EVERYWHERE: Bounds = Bounds {
        min: [f64::NEG_INFINITY; 2],
        max: [f64::INFINITY; 2],
    };

    pub fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Bounds {
        Bounds {
            min: [x0.min(x1), y0.min(y1)],
            max: [x0.max(x1), y0.max(y1)],
        }
    }

    pub fn is_empty(&self) -> bool {
        self.min[0] > self.max[0] || self.min[1] > self.max[1]
    }

    pub fn include(&mut self, p: [f64; 2]) {
        for (k, v) in p.iter().enumerate() {
            if *v < self.min[k] {
                self.min[k] = *v;
            }
            if *v > self.max[k] {
                self.max[k] = *v;
            }
        }
    }

    pub fn union(&mut self, o: &Bounds) {
        if o.is_empty() {
            return;
        }
        self.include(o.min);
        self.include(o.max);
    }

    pub fn intersects(&self, o: &Bounds) -> bool {
        !self.is_empty()
            && !o.is_empty()
            && self.min[0] <= o.max[0]
            && o.min[0] <= self.max[0]
            && self.min[1] <= o.max[1]
            && o.min[1] <= self.max[1]
    }

    /// Grown by `d` on every side.
    pub fn grown(&self, d: f64) -> Bounds {
        if self.is_empty() {
            return *self;
        }
        Bounds {
            min: [self.min[0] - d, self.min[1] - d],
            max: [self.max[0] + d, self.max[1] + d],
        }
    }

    pub fn width(&self) -> f64 {
        self.max[0] - self.min[0]
    }

    pub fn height(&self) -> f64 {
        self.max[1] - self.min[1]
    }

    pub fn center(&self) -> [f64; 2] {
        [
            (self.min[0] + self.max[0]) / 2.0,
            (self.min[1] + self.max[1]) / 2.0,
        ]
    }

    pub fn is_finite(&self) -> bool {
        self.min
            .iter()
            .chain(self.max.iter())
            .all(|v| v.is_finite())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// `#rrggbb`.
    pub fn from_hex(s: &str) -> Option<Rgb> {
        let h = s.strip_prefix('#')?;
        if h.len() != 6 || !h.is_ascii() {
            return None;
        }
        let v = u32::from_str_radix(h, 16).ok()?;
        Some(Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
    }
}

/// How an item is drawn: its colour on the dark canvas and on paper, and
/// its line width in pixels (DESIGN.md §3).
#[derive(Debug, Clone, PartialEq)]
pub struct Style {
    pub screen: Rgb,
    pub print: Rgb,
    pub px: f64,
    pub layer: String,
    pub linetype: String,
}

/// One drawing operation, world coordinates.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Line([f64; 4]),
    /// Counter-clockwise from `start` to `end`, radians (a circle is 0 to 2π).
    Arc {
        center: [f64; 2],
        radius: f64,
        start: f64,
        end: f64,
    },
    Path {
        closed: bool,
        points: Vec<[f64; 2]>,
    },
    /// Even-odd fill of the rings.
    Fill(Vec<Vec<[f64; 2]>>),
    Dot([f64; 2]),
    /// A construction line through `origin` along the unit vector `dir`, both
    /// ways or (`ray`) forward only, dashed by `dashes` (world units:
    /// positive dash, negative gap, 0 dot; empty is solid).
    Ray {
        origin: [f64; 2],
        dir: [f64; 2],
        ray: bool,
        dashes: Vec<f64>,
    },
}

impl Op {
    /// Where the operation can put ink; a construction line has no ends, so
    /// it is everywhere.
    pub fn bounds(&self) -> Bounds {
        let mut b = Bounds::EMPTY;
        match self {
            Op::Line(l) => {
                b.include([l[0], l[1]]);
                b.include([l[2], l[3]]);
            }
            Op::Arc { center, radius, .. } => {
                b.include([center[0] - radius, center[1] - radius]);
                b.include([center[0] + radius, center[1] + radius]);
            }
            Op::Path { points, .. } => points.iter().for_each(|p| b.include(*p)),
            Op::Fill(rings) => rings.iter().flatten().for_each(|p| b.include(*p)),
            Op::Dot(p) => b.include(*p),
            Op::Ray { .. } => return Bounds::EVERYWHERE,
        }
        b
    }
}

/// The operations of one entity, with the style they share.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub handle: u64,
    pub style: usize,
    pub ops: Vec<Op>,
    /// Where the item can put ink, to skip it when it is off screen.
    pub bounds: Bounds,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Display {
    /// The extent the core reported (`None` for an empty drawing).
    pub bounds: Option<Bounds>,
    pub styles: Vec<Style>,
    pub items: Vec<Item>,
    /// What the core left out and how many, by reason.
    pub skipped: Vec<(String, f64)>,
}

impl Display {
    /// The extent of what is drawn: the core's, else computed from the items
    /// that have ends.
    pub fn extents(&self) -> Option<Bounds> {
        if let Some(b) = self.bounds.filter(|b| b.is_finite() && !b.is_empty()) {
            return Some(b);
        }
        let mut b = Bounds::EMPTY;
        for it in &self.items {
            if it.bounds.is_finite() {
                b.union(&it.bounds);
            }
        }
        (!b.is_empty()).then_some(b)
    }

    /// Indices of the items of entity `handle`.
    pub fn items_of(&self, handle: u64) -> impl Iterator<Item = usize> + '_ {
        self.items
            .iter()
            .enumerate()
            .filter(move |(_, it)| it.handle == handle)
            .map(|(i, _)| i)
    }

    pub fn parse(bytes: &[u8]) -> Result<Display, String> {
        if bytes.get(..4) != Some(MAGIC.as_slice()) {
            return Err("not a display list".into());
        }
        let u32_at = |at: usize| -> Option<u32> {
            bytes
                .get(at..at + 4)?
                .try_into()
                .ok()
                .map(u32::from_le_bytes)
        };
        if u32_at(4) != Some(LAYOUT_VERSION) {
            return Err("display list: unknown layout version".into());
        }
        let n = u32_at(8).ok_or("display list: short header")? as usize;
        let head_end = 12usize
            .checked_add(n)
            .filter(|e| *e <= bytes.len())
            .ok_or("display list: the header runs past the end")?;
        let head_text = std::str::from_utf8(&bytes[12..head_end])
            .map_err(|_| "display list: the header is not UTF-8")?;
        let head = Json::parse(head_text)?;
        let (bounds, styles, skipped) = read_header(&head)?;
        let start = head_end.div_ceil(8) * 8;
        let body = bytes.get(start..).unwrap_or(&[]);
        if body.len() % 8 != 0 {
            return Err("display list: the operations are not whole numbers".into());
        }
        let vals: Vec<f64> = body
            .chunks_exact(8)
            .map(|c| f64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
            .collect();
        let items = read_ops(&vals, styles.len())?;
        Ok(Display {
            bounds,
            styles,
            items,
            skipped,
        })
    }
}

type Header = (Option<Bounds>, Vec<Style>, Vec<(String, f64)>);

fn read_header(head: &Json) -> Result<Header, String> {
    let bounds = match head.get("bounds") {
        Some(Json::Arr(a)) if a.len() == 4 => {
            let v: Vec<f64> = a.iter().filter_map(Json::as_f64).collect();
            (v.len() == 4).then(|| Bounds::new(v[0], v[1], v[2], v[3]))
        }
        _ => None,
    };
    let mut styles = Vec::new();
    for s in head
        .get("styles")
        .and_then(Json::as_array)
        .ok_or("display list: no style table")?
    {
        styles.push(Style {
            screen: s
                .get("screen")
                .and_then(Json::as_str)
                .and_then(Rgb::from_hex)
                .unwrap_or(Rgb(255, 255, 255)),
            print: s
                .get("print")
                .and_then(Json::as_str)
                .and_then(Rgb::from_hex)
                .unwrap_or(Rgb(0, 0, 0)),
            px: s
                .get("px")
                .and_then(Json::as_f64)
                .filter(|p| p.is_finite() && *p >= 0.0)
                .unwrap_or(1.0),
            layer: s
                .get("layer")
                .and_then(Json::as_str)
                .unwrap_or("")
                .to_string(),
            linetype: s
                .get("linetype")
                .and_then(Json::as_str)
                .unwrap_or("")
                .to_string(),
        });
    }
    let skipped = match head.get("skipped") {
        Some(Json::Obj(m)) => m
            .iter()
            .filter_map(|(k, v)| v.as_f64().map(|n| (k.clone(), n)))
            .collect(),
        _ => Vec::new(),
    };
    Ok((bounds, styles, skipped))
}

/// A cursor over the operation values.
struct Ops<'a> {
    v: &'a [f64],
    i: usize,
}

impl Ops<'_> {
    fn left(&self) -> usize {
        self.v.len().saturating_sub(self.i)
    }

    fn next(&mut self) -> Result<f64, String> {
        let x = *self
            .v
            .get(self.i)
            .ok_or_else(|| "display list: an operation is cut short".to_string())?;
        self.i += 1;
        Ok(x)
    }

    fn finite(&mut self) -> Result<f64, String> {
        let x = self.next()?;
        if x.is_finite() {
            Ok(x)
        } else {
            Err("display list: a coordinate is not a number".into())
        }
    }

    /// A count that the values left could hold at `per` values each.
    fn count(&mut self, per: usize) -> Result<usize, String> {
        let x = self.next()?;
        if x.is_finite() && x >= 0.0 && x.fract() == 0.0 && x <= (self.left() / per.max(1)) as f64 {
            Ok(x as usize)
        } else {
            Err("display list: a count runs past the end".into())
        }
    }

    fn point(&mut self) -> Result<[f64; 2], String> {
        Ok([self.finite()?, self.finite()?])
    }

    fn points(&mut self, n: usize) -> Result<Vec<[f64; 2]>, String> {
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(self.point()?);
        }
        Ok(out)
    }
}

fn read_ops(vals: &[f64], styles: usize) -> Result<Vec<Item>, String> {
    let mut r = Ops { v: vals, i: 0 };
    let mut items: Vec<Item> = Vec::new();
    while r.left() > 0 {
        let code = r.next()?;
        if code == 1.0 {
            let h = r.next()?;
            let s = r.next()?;
            if !(h.is_finite() && h >= 0.0 && h.fract() == 0.0 && h <= 9_007_199_254_740_992.0) {
                return Err("display list: a handle is not a whole number".into());
            }
            if !(s.is_finite() && s >= 0.0 && s.fract() == 0.0 && (s as usize) < styles) {
                return Err("display list: a style is not in the table".into());
            }
            items.push(Item {
                handle: h as u64,
                style: s as usize,
                ops: Vec::new(),
                bounds: Bounds::EMPTY,
            });
            continue;
        }
        let op = read_op(&mut r, code)?;
        let item = items
            .last_mut()
            .ok_or("display list: an operation before any item")?;
        item.bounds.union(&op.bounds());
        item.ops.push(op);
    }
    Ok(items)
}

fn read_op(r: &mut Ops<'_>, code: f64) -> Result<Op, String> {
    Ok(match code {
        2.0 => Op::Line([r.finite()?, r.finite()?, r.finite()?, r.finite()?]),
        3.0 => {
            let center = r.point()?;
            let radius = r.finite()?;
            let (start, end) = (r.finite()?, r.finite()?);
            Op::Arc {
                center,
                radius: radius.abs(),
                start,
                end,
            }
        }
        4.0 => {
            let n = r.count(2)?;
            let closed = r.next()? != 0.0;
            Op::Path {
                closed,
                points: r.points(n)?,
            }
        }
        5.0 => {
            let rings = r.count(1)?;
            let mut out = Vec::with_capacity(rings);
            for _ in 0..rings {
                let n = r.count(2)?;
                out.push(r.points(n)?);
            }
            Op::Fill(out)
        }
        6.0 => Op::Dot(r.point()?),
        7.0 => {
            let origin = r.point()?;
            let dir = r.point()?;
            let ray = r.next()? != 0.0;
            let n = r.count(1)?;
            let mut dashes = Vec::with_capacity(n);
            for _ in 0..n {
                dashes.push(r.finite()?);
            }
            Op::Ray {
                origin,
                dir,
                ray,
                dashes,
            }
        }
        _ => return Err("display list: an unknown operation".into()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A list built by hand: header JSON, padding, then values.
    fn list(head: &str, vals: &[f64]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(MAGIC);
        b.extend_from_slice(&LAYOUT_VERSION.to_le_bytes());
        b.extend_from_slice(&(head.len() as u32).to_le_bytes());
        b.extend_from_slice(head.as_bytes());
        while b.len() % 8 != 0 {
            b.push(0);
        }
        for v in vals {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b
    }

    const HEAD: &str = r##"{"bounds":[0,0,10,5],"styles":[{"screen":"#ff0000","print":"#00ff00","px":2,"layer":"VISIBLE","linetype":"CONTINUOUS"}],"items":2,"skipped":{"proxy":3}}"##;

    #[test]
    fn decodes_every_operation() {
        let vals = [
            1.0, 47.0, 0.0, // item 0x2F, style 0
            2.0, 0.0, 0.0, 10.0, 0.0, // line
            3.0, 5.0, 5.0, 2.0, 0.0, 6.0, // arc
            4.0, 3.0, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0, // closed path of 3
            5.0, 1.0, 3.0, 0.0, 0.0, 4.0, 0.0, 4.0, 4.0, // a fill, one ring
            6.0, 2.0, 3.0, // dot
            7.0, 0.0, 0.0, 1.0, 0.0, 0.0, 2.0, 5.0, -2.0, // ray with a pattern
        ];
        let d = Display::parse(&list(HEAD, &vals)).unwrap();
        assert_eq!(d.styles.len(), 1);
        assert_eq!(d.styles[0].screen, Rgb(255, 0, 0));
        assert_eq!(d.styles[0].px, 2.0);
        assert_eq!(d.styles[0].layer, "VISIBLE");
        assert_eq!(d.skipped, vec![("proxy".to_string(), 3.0)]);
        assert_eq!(d.items.len(), 1);
        let it = &d.items[0];
        assert_eq!((it.handle, it.style, it.ops.len()), (0x2F, 0, 6));
        assert!(matches!(it.ops[1], Op::Arc { radius, .. } if radius == 2.0));
        assert!(matches!(&it.ops[2], Op::Path { closed: true, points } if points.len() == 3));
        assert!(matches!(&it.ops[5], Op::Ray { ray: false, dashes, .. } if dashes == &[5.0, -2.0]));
        assert_eq!(
            it.bounds,
            Bounds::EVERYWHERE,
            "a construction line reaches everywhere"
        );
        assert_eq!(d.extents(), Some(Bounds::new(0.0, 0.0, 10.0, 5.0)));
        assert_eq!(d.items_of(0x2F).count(), 1);
    }

    #[test]
    fn item_bounds_cover_the_ink() {
        let vals = [
            1.0, 1.0, 0.0, 2.0, -1.0, 2.0, 3.0, 4.0, 3.0, 0.0, 0.0, 2.0, 0.0, 6.0,
        ];
        let d = Display::parse(&list(HEAD, &vals)).unwrap();
        assert_eq!(d.items[0].bounds, Bounds::new(-2.0, -2.0, 3.0, 4.0));
    }

    #[test]
    fn extents_fall_back_to_the_items() {
        let head = r#"{"bounds":null,"styles":[{}],"items":1}"#;
        let d = Display::parse(&list(head, &[1.0, 1.0, 0.0, 2.0, 1.0, 1.0, 3.0, 5.0])).unwrap();
        assert_eq!(d.bounds, None);
        assert_eq!(d.extents(), Some(Bounds::new(1.0, 1.0, 3.0, 5.0)));
        let empty = Display::parse(&list(head, &[])).unwrap();
        assert_eq!(empty.extents(), None);
    }

    #[test]
    fn refuses_bad_lists() {
        let ok = list(HEAD, &[1.0, 1.0, 0.0, 2.0, 0.0, 0.0, 1.0, 1.0]);
        assert!(Display::parse(&ok).is_ok());
        assert!(Display::parse(b"").is_err());
        assert!(Display::parse(b"NOPE\x01\0\0\0\0\0\0\0").is_err());
        // a style the table does not have
        assert!(Display::parse(&list(HEAD, &[1.0, 1.0, 5.0])).is_err());
        // an operation before any item
        assert!(Display::parse(&list(HEAD, &[2.0, 0.0, 0.0, 1.0, 1.0])).is_err());
        // an unknown operation
        assert!(Display::parse(&list(HEAD, &[1.0, 1.0, 0.0, 99.0])).is_err());
        // a count far larger than the bytes
        assert!(Display::parse(&list(HEAD, &[1.0, 1.0, 0.0, 4.0, 1.0e18, 0.0, 1.0, 1.0])).is_err());
        assert!(Display::parse(&list(HEAD, &[1.0, 1.0, 0.0, 5.0, 1.0e18])).is_err());
        assert!(
            Display::parse(&list(
                HEAD,
                &[1.0, 1.0, 0.0, 7.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0e18]
            ))
            .is_err()
        );
        // NaN and infinity are not coordinates
        assert!(
            Display::parse(&list(HEAD, &[1.0, 1.0, 0.0, 2.0, f64::NAN, 0.0, 1.0, 1.0])).is_err()
        );
        assert!(Display::parse(&list(HEAD, &[1.0, 1.0, 0.0, 6.0, f64::INFINITY, 0.0])).is_err());
        // a handle that is not whole
        assert!(Display::parse(&list(HEAD, &[1.0, 1.5, 0.0])).is_err());
        // a header that runs past the end
        let mut cut = ok.clone();
        cut[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(Display::parse(&cut).is_err());
        // values that are not whole numbers of 8 bytes
        let mut ragged = ok.clone();
        ragged.push(0);
        assert!(Display::parse(&ragged).is_err());
    }

    #[test]
    fn no_prefix_of_a_list_panics() {
        let vals = [
            1.0, 1.0, 0.0, 2.0, 0.0, 0.0, 1.0, 1.0, 4.0, 2.0, 0.0, 0.0, 0.0, 1.0, 1.0, 5.0, 1.0,
            3.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0,
        ];
        let full = list(HEAD, &vals);
        for n in 0..full.len() {
            let _ = Display::parse(&full[..n]);
        }
        assert!(Display::parse(&full).is_ok());
    }

    #[test]
    fn colours() {
        assert_eq!(Rgb::from_hex("#0a0b0c"), Some(Rgb(10, 11, 12)));
        for bad in ["0a0b0c", "#0a0b0", "#0a0b0cc", "#gggggg", "#é0b0c0"] {
            assert_eq!(Rgb::from_hex(bad), None, "{bad}");
        }
    }
}
