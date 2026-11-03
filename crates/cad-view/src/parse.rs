//! Reading a display list (`cad_core::export::display`).

use crate::json::{self, Json};
use crate::Error;

const MAGIC: &[u8; 4] = b"DSPL";
const LAYOUT_VERSION: u32 = 1;

const OP_ITEM: f64 = 1.0;
const OP_LINE: f64 = 2.0;
const OP_ARC: f64 = 3.0;
const OP_PATH: f64 = 4.0;
const OP_FILL: f64 = 5.0;
const OP_DOT: f64 = 6.0;
const OP_RAY: f64 = 7.0;

/// How a kind of item is drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct Style {
    /// Colour on the dark canvas.
    pub screen: [u8; 3],
    /// Colour printed in colour.
    pub print: [u8; 3],
    /// Line width in pixels (points); 1 or less is a hairline.
    pub px: f32,
    pub layer: String,
    pub linetype: String,
}

/// One entity's drawing: the operations `ops[start..end]` of the list.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    /// The entity (the insert, for block contents).
    pub handle: u64,
    pub style: u32,
    pub start: usize,
    pub end: usize,
    /// `[x0, y0, x1, y1]`; empty (min > max) for an item with no extent (a construction line).
    pub bbox: [f64; 4],
}

/// One operation, borrowed from the list.
#[derive(Debug, Clone, PartialEq)]
pub enum Op<'a> {
    Line([f64; 4]),
    /// Counter-clockwise, radians; a full circle is 0 to 2π.
    Arc { c: [f64; 2], r: f64, a0: f64, a1: f64 },
    /// `pts` is x, y pairs.
    Path { pts: &'a [f64], closed: bool },
    /// Even-odd rings, each x, y pairs.
    Fill { rings: Vec<&'a [f64]> },
    Dot([f64; 2]),
    /// A construction line through `o` along unit `d`; `pattern` is the dash pattern (empty: solid).
    Ray { o: [f64; 2], d: [f64; 2], ray: bool, pattern: &'a [f64] },
}

#[derive(Debug, Clone, Default)]
pub struct Display {
    /// The whole drawing's extent.
    pub bounds: Option<[f64; 4]>,
    pub styles: Vec<Style>,
    pub items: Vec<Item>,
    pub ops: Vec<f64>,
    /// Entities the core could not draw, by reason.
    pub skipped: Vec<(String, u64)>,
    /// Set on a patch: the handles whose items it replaces.
    pub patch: Option<Vec<u64>>,
}

fn empty_box() -> [f64; 4] {
    [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY]
}

fn grow(b: &mut [f64; 4], x: f64, y: f64) {
    if x.is_finite() && y.is_finite() {
        b[0] = b[0].min(x);
        b[1] = b[1].min(y);
        b[2] = b[2].max(x);
        b[3] = b[3].max(y);
    }
}

fn hex3(s: &str) -> Option<[u8; 3]> {
    let s = s.strip_prefix('#')?;
    if s.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

/// The operation at `i`: what it is and where the next starts.
pub(crate) fn decode(ops: &[f64], i: usize) -> Option<(Op<'_>, usize)> {
    let code = *ops.get(i)?;
    let n = |k: usize| ops.get(i + k).copied();
    let count = |v: f64| -> Option<usize> { (v.is_finite() && v >= 0.0 && v <= (ops.len() as f64)).then_some(v as usize) };
    if code == OP_LINE {
        let s = ops.get(i + 1..i + 5)?;
        return Some((Op::Line([s[0], s[1], s[2], s[3]]), i + 5));
    }
    if code == OP_ARC {
        let s = ops.get(i + 1..i + 6)?;
        return Some((Op::Arc { c: [s[0], s[1]], r: s[2], a0: s[3], a1: s[4] }, i + 6));
    }
    if code == OP_PATH {
        let k = count(n(1)?)?;
        let closed = n(2)? != 0.0;
        let end = (i + 3).checked_add(k.checked_mul(2)?)?;
        return Some((Op::Path { pts: ops.get(i + 3..end)?, closed }, end));
    }
    if code == OP_FILL {
        let rings = count(n(1)?)?;
        let mut at = i + 2;
        let mut out = Vec::new();
        for _ in 0..rings {
            let k = count(*ops.get(at)?)?;
            let end = (at + 1).checked_add(k.checked_mul(2)?)?;
            out.push(ops.get(at + 1..end)?);
            at = end;
        }
        return Some((Op::Fill { rings: out }, at));
    }
    if code == OP_DOT {
        let s = ops.get(i + 1..i + 3)?;
        return Some((Op::Dot([s[0], s[1]]), i + 3));
    }
    if code == OP_RAY {
        let s = ops.get(i + 1..i + 7)?;
        let k = count(s[5])?;
        let end = (i + 7).checked_add(k)?;
        return Some((Op::Ray { o: [s[0], s[1]], d: [s[2], s[3]], ray: s[4] != 0.0, pattern: ops.get(i + 7..end)? }, end));
    }
    None
}

impl Display {
    /// Read the bytes of a display list or patch.
    pub fn parse(bytes: &[u8]) -> Result<Display, Error> {
        if bytes.len() < 12 || bytes.get(..4) != Some(MAGIC) {
            return Err(Error::NotADisplayList("wrong magic".into()));
        }
        let u32_at = |o: usize| -> Option<u32> { Some(u32::from_le_bytes(bytes.get(o..o + 4)?.try_into().ok()?)) };
        if u32_at(4) != Some(LAYOUT_VERSION) {
            return Err(Error::NotADisplayList("unknown layout version".into()));
        }
        let jlen = u32_at(8).ok_or_else(|| Error::Header("short".into()))? as usize;
        let jend = 12usize.checked_add(jlen).filter(|e| *e <= bytes.len()).ok_or_else(|| Error::Header("header runs past the data".into()))?;
        let text = std::str::from_utf8(bytes.get(12..jend).unwrap_or_default()).map_err(|_| Error::Header("not UTF-8".into()))?;
        let head = json::parse(text)?;
        let start = jend.div_ceil(8) * 8;
        let body = bytes.get(start.min(bytes.len())..).unwrap_or_default();
        if body.len() % 8 != 0 {
            return Err(Error::Format("operations are not whole numbers".into()));
        }
        let ops: Vec<f64> = body.chunks_exact(8).filter_map(|c| c.try_into().ok().map(f64::from_le_bytes)).collect();

        let bounds = head.get("bounds").and_then(Json::as_arr).and_then(|a| {
            let v: Vec<f64> = a.iter().filter_map(Json::as_f64).collect();
            (v.len() == 4 && v.iter().all(|x| x.is_finite())).then(|| [v[0], v[1], v[2], v[3]])
        });
        let mut styles = Vec::new();
        for s in head.get("styles").and_then(Json::as_arr).unwrap_or_default() {
            styles.push(Style {
                screen: s.get("screen").and_then(Json::as_str).and_then(hex3).unwrap_or([255, 255, 255]),
                print: s.get("print").and_then(Json::as_str).and_then(hex3).unwrap_or([0, 0, 0]),
                px: s.get("px").and_then(Json::as_f64).filter(|p| p.is_finite()).unwrap_or(1.0).clamp(0.0, 64.0) as f32,
                layer: s.get("layer").and_then(Json::as_str).unwrap_or_default().to_string(),
                linetype: s.get("linetype").and_then(Json::as_str).unwrap_or_default().to_string(),
            });
        }
        let skipped = match head.get("skipped") {
            Some(Json::Obj(m)) => m.iter().filter_map(|(k, v)| Some((k.clone(), v.as_f64()? as u64))).collect(),
            _ => Vec::new(),
        };
        let patch = head.get("patch").and_then(Json::as_arr).map(|a| {
            a.iter().filter_map(|h| u64::from_str_radix(h.as_str()?, 16).ok()).collect::<Vec<u64>>()
        });

        // Walk the operations into items.
        let mut items: Vec<Item> = Vec::new();
        let mut i = 0;
        while i < ops.len() {
            if ops[i] != OP_ITEM {
                return Err(Error::Format(format!("operation {} is not an item start", ops[i])));
            }
            let handle = ops.get(i + 1).copied().filter(|h| h.is_finite() && *h >= 0.0).ok_or_else(|| Error::Format("item without a handle".into()))? as u64;
            let style = ops.get(i + 2).copied().filter(|s| s.is_finite() && *s >= 0.0).ok_or_else(|| Error::Format("item without a style".into()))? as u32;
            if styles.get(style as usize).is_none() {
                return Err(Error::Format("item names a style that is not in the table".into()));
            }
            let start = i + 3;
            let mut at = start;
            let mut bbox = empty_box();
            while at < ops.len() && ops[at] != OP_ITEM {
                let (op, next) = decode(&ops, at).ok_or_else(|| Error::Format("an operation ends early or is unknown".into()))?;
                match op {
                    Op::Line(l) => {
                        grow(&mut bbox, l[0], l[1]);
                        grow(&mut bbox, l[2], l[3]);
                    }
                    Op::Arc { c, r, .. } => {
                        grow(&mut bbox, c[0] - r, c[1] - r);
                        grow(&mut bbox, c[0] + r, c[1] + r);
                    }
                    Op::Path { pts, .. } => pts.chunks_exact(2).for_each(|p| grow(&mut bbox, p[0], p[1])),
                    Op::Fill { rings } => rings.iter().for_each(|r| r.chunks_exact(2).for_each(|p| grow(&mut bbox, p[0], p[1]))),
                    Op::Dot(p) => grow(&mut bbox, p[0], p[1]),
                    Op::Ray { .. } => {}
                }
                at = next;
            }
            items.push(Item { handle, style, start, end: at, bbox });
            i = at;
        }
        Ok(Display { bounds, styles, items, ops, skipped, patch })
    }

    /// Apply a patch (a display list with `patch` set) to this list: the items of its handles
    /// are dropped and the patch's items added; bounds and skipped are the patch's (the whole
    /// scene's).
    pub fn apply_patch(&mut self, patch: Display) {
        let Some(handles) = &patch.patch else {
            *self = patch;
            return;
        };
        let drop: std::collections::BTreeSet<u64> = handles.iter().copied().collect();
        let mut ops = Vec::with_capacity(self.ops.len() + patch.ops.len());
        let mut items = Vec::with_capacity(self.items.len() + patch.items.len());
        for it in &self.items {
            if drop.contains(&it.handle) {
                continue;
            }
            let start = ops.len();
            ops.extend_from_slice(self.ops.get(it.start..it.end).unwrap_or_default());
            items.push(Item { start, end: ops.len(), ..it.clone() });
        }
        let mut remap: Vec<u32> = Vec::with_capacity(patch.styles.len());
        for s in &patch.styles {
            let at = match self.styles.iter().position(|e| e == s) {
                Some(k) => k,
                None => {
                    self.styles.push(s.clone());
                    self.styles.len() - 1
                }
            };
            remap.push(at as u32);
        }
        for it in &patch.items {
            let start = ops.len();
            ops.extend_from_slice(patch.ops.get(it.start..it.end).unwrap_or_default());
            items.push(Item { start, end: ops.len(), style: remap.get(it.style as usize).copied().unwrap_or(0), ..it.clone() });
        }
        self.ops = ops;
        self.items = items;
        self.bounds = patch.bounds;
        self.skipped = patch.skipped;
    }

    /// The operations of an item.
    pub fn item_ops<'a>(&'a self, item: &Item) -> impl Iterator<Item = Op<'a>> + 'a {
        let ops: &'a [f64] = self.ops.get(item.start..item.end).unwrap_or_default();
        let mut at = 0;
        std::iter::from_fn(move || {
            let (op, next) = decode(ops, at)?;
            at = next;
            Some(op)
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Encode a display list by hand (what `cad_core::export::display` writes).
    pub fn encode(json: &str, ops: &[f64]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&LAYOUT_VERSION.to_le_bytes());
        out.extend_from_slice(&(json.len() as u32).to_le_bytes());
        out.extend_from_slice(json.as_bytes());
        while out.len() % 8 != 0 {
            out.push(0);
        }
        for v in ops {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out
    }

    pub const HEAD: &str = r##"{"bounds":[0,0,10,10],"styles":[{"screen":"#ffffff","print":"#000000","px":1,"layer":"0","linetype":""},{"screen":"#ff0000","print":"#880000","px":3,"layer":"A","linetype":"HIDDEN"}],"items":2,"skipped":{"proxy":2}}"##;

    #[test]
    fn reads_every_operation() {
        let ops = [
            1.0, 42.0, 0.0, 2.0, 0.0, 0.0, 10.0, 0.0, // item, line
            3.0, 5.0, 5.0, 2.0, 0.0, 6.0, // arc
            4.0, 3.0, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0, // closed path of 3
            1.0, 43.0, 1.0, 5.0, 2.0, 4.0, 0.0, 0.0, 4.0, 0.0, 4.0, 4.0, 0.0, 4.0, 4.0, 1.0, 1.0, 2.0, 1.0, 2.0, 2.0, 1.0, 2.0, // fill: outer square, inner
            6.0, 3.0, 3.0, 7.0, 0.0, 0.0, 1.0, 0.0, 1.0, 2.0, 4.0, 1.0,
        ];
        let d = Display::parse(&encode(HEAD, &ops)).unwrap();
        assert_eq!(d.items.len(), 2);
        assert_eq!(d.items[0].handle, 42);
        assert_eq!(d.items[1].style, 1);
        assert_eq!(d.styles[1].px, 3.0);
        assert_eq!(d.styles[1].screen, [255, 0, 0]);
        assert_eq!(d.skipped, vec![("proxy".to_string(), 2)]);
        let a: Vec<Op> = d.item_ops(&d.items[0]).collect();
        assert_eq!(a.len(), 3);
        assert_eq!(a[0], Op::Line([0.0, 0.0, 10.0, 0.0]));
        let b: Vec<Op> = d.item_ops(&d.items[1]).collect();
        match &b[0] {
            Op::Fill { rings } => assert_eq!((rings.len(), rings[0].len(), rings[1].len()), (2, 8, 8)),
            o => panic!("{o:?}"),
        }
        assert!(matches!(b[1], Op::Dot([3.0, 3.0])));
        assert!(matches!(&b[2], Op::Ray { ray: true, pattern, .. } if pattern == &[4.0, 1.0]));
        assert_eq!(d.items[0].bbox[2], 10.0);
    }

    #[test]
    fn damaged_lists_are_errors() {
        let good = encode(HEAD, &[1.0, 1.0, 0.0, 2.0, 0.0, 0.0, 1.0, 1.0]);
        assert!(Display::parse(&good).is_ok());
        assert!(Display::parse(b"").is_err());
        assert!(Display::parse(b"NOPE\x01\0\0\0\0\0\0\0").is_err());
        for cut in 0..good.len() {
            let _ = Display::parse(&good[..cut]); // never a panic
        }
        // a style that is not in the table
        assert!(Display::parse(&encode(HEAD, &[1.0, 1.0, 9.0])).is_err());
        // an operation that ends early
        assert!(Display::parse(&encode(HEAD, &[1.0, 1.0, 0.0, 2.0, 0.0, 0.0])).is_err());
        // a count past the data
        assert!(Display::parse(&encode(HEAD, &[1.0, 1.0, 0.0, 4.0, 1e18, 0.0])).is_err());
        assert!(Display::parse(&encode(HEAD, &[1.0, 1.0, 0.0, 5.0, f64::NAN])).is_err());
        // something that is not an item
        assert!(Display::parse(&encode(HEAD, &[2.0, 0.0, 0.0, 1.0, 1.0])).is_err());
    }

    #[test]
    fn a_patch_replaces_the_items_of_its_handles() {
        let mut d = Display::parse(&encode(
            HEAD,
            &[1.0, 1.0, 0.0, 2.0, 0.0, 0.0, 1.0, 0.0, 1.0, 2.0, 0.0, 2.0, 0.0, 2.0, 0.0, 0.0, 2.0, 0.0, 3.0, 1.0, 5.0],
        ))
        .unwrap();
        assert_eq!(d.items.len(), 2);
        let patch_head = r##"{"bounds":[0,0,20,20],"styles":[{"screen":"#00ff00","print":"#008800","px":1,"layer":"N","linetype":""}],"items":2,"skipped":{},"patch":["1","7"]}"##;
        let p = Display::parse(&encode(patch_head, &[1.0, 1.0, 0.0, 2.0, 9.0, 9.0, 19.0, 19.0])).unwrap();
        assert_eq!(p.patch, Some(vec![1, 7]));
        d.apply_patch(p);
        // handle 1 now draws the new line; handle 2's item is untouched; the style was added
        assert_eq!(d.items.len(), 2);
        let h1 = d.items.iter().find(|i| i.handle == 1).unwrap();
        assert_eq!(d.styles[h1.style as usize].layer, "N");
        assert_eq!(d.item_ops(h1).next(), Some(Op::Line([9.0, 9.0, 19.0, 19.0])));
        let h2 = d.items.iter().find(|i| i.handle == 2).unwrap();
        assert_eq!(d.item_ops(h2).count(), 2);
        assert_eq!(d.bounds, Some([0.0, 0.0, 20.0, 20.0]));
        // an empty patch drops nothing
        let none = Display::parse(&encode(r##"{"bounds":null,"styles":[],"items":0,"skipped":{},"patch":[]}"##, &[])).unwrap();
        let n = d.items.len();
        d.apply_patch(none);
        assert_eq!(d.items.len(), n);
    }
}
