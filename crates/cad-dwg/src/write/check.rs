//! Reading a written file back and comparing it with the drawing it was
//! written from, object by object.
//!
//! The writer reports what it could not encode (`DwgOutput::left_out`). A
//! file can still lose more than that without the writer knowing: a field
//! written in the wrong place, or one the model holds and the encoder never
//! writes, reads back as a different value. Counting objects does not see
//! that; comparing them does.

use super::{dxf_only_objects, without_dxf_only, DwgOutput};
use crate::model::*;
use std::collections::BTreeMap;

/// How a written file compares with the drawing it was written from.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Check {
    /// Objects that read back different from what was written, by type.
    pub changed: BTreeMap<String, usize>,
    /// Objects that did not read back at all, beyond those the writer
    /// reported leaving out, by type.
    pub vanished: BTreeMap<String, usize>,
    /// What differed, for people to read: the first few objects, each with
    /// the first field that differs.
    pub notes: Vec<String>,
}

impl Check {
    /// True when every object the writer kept reads back as it was.
    pub fn is_whole(&self) -> bool {
        self.changed.is_empty() && self.vanished.is_empty()
    }
}

/// Most objects described in [`Check::notes`].
const NOTES: usize = 8;

/// Read `out` back and compare it with `d`, the drawing it was written
/// from.
///
/// Objects the writer reported leaving out, objects kept only as their
/// original file records, and the objects AutoCAD keeps in DXF but not in
/// DWG are not compared. For each other object the type, owner, extension
/// dictionary, reactors, entity properties, extended data and everything
/// the model decodes must come back equal. Header variables are not
/// compared.
///
/// An error means the written file could not be read at all.
pub fn check(d: &Drawing, out: &DwgOutput) -> crate::error::Result<Check> {
    let back = crate::read_bytes(&out.bytes)?;
    let dxf_only = dxf_only_objects(d);
    let mut c = Check::default();
    let mut missing: BTreeMap<String, usize> = BTreeMap::new();
    for o in d.objects.values() {
        if dxf_only.contains(&o.handle) || o.raw.is_some() {
            continue;
        }
        let written = without_dxf_only(o, &dxf_only);
        let o = written.as_ref().unwrap_or(o);
        let Some(b) = back.objects.get(&o.handle) else {
            *missing.entry(o.type_name.clone()).or_default() += 1;
            continue;
        };
        if let Some(field) = first_difference(o, b) {
            *c.changed.entry(o.type_name.clone()).or_default() += 1;
            if c.notes.len() < NOTES {
                c.notes.push(format!("{:X} {}: {field} reads back different", o.handle.0, o.type_name));
            }
        }
    }
    for (t, n) in missing {
        let reported = out.left_out.get(&t).copied().unwrap_or(0);
        if n > reported {
            c.vanished.insert(t.clone(), n - reported);
            if c.notes.len() < NOTES {
                c.notes.push(format!("{} {t} missing from the file", n - reported));
            }
        }
    }
    Ok(c)
}

/// The first part of `a` that `b` does not hold the same, or `None`.
fn first_difference(a: &Object, b: &Object) -> Option<&'static str> {
    if a.type_name != b.type_name {
        Some("its type")
    } else if a.owner != b.owner {
        Some("its owner")
    } else if a.xdictionary != b.xdictionary {
        Some("its extension dictionary")
    } else if a.reactors != b.reactors {
        Some("its reactors")
    } else if a.entity != b.entity {
        Some("its entity properties")
    } else if a.xdata != b.xdata {
        Some("its extended data")
    } else if !same_data(&a.kind, &b.kind) {
        Some("its data")
    } else {
        None
    }
}

/// Whether `b` holds everything `a` does. An object kept as its DXF
/// groups may come back with groups the file's version adds (a 2018 file
/// stores fields an older DXF had no place for), and in the order the
/// reader gives them, which is AutoCAD's: a table style's border groups,
/// for one, come back row by row rather than kind by kind. It holds the
/// same as long as every group written comes back, as often as it was
/// written.
fn same_data(a: &ObjectKind, b: &ObjectKind) -> bool {
    match (a, b) {
        (ObjectKind::NonGraphic(NonGraphic::Groups(ga)), ObjectKind::NonGraphic(NonGraphic::Groups(gb))) => {
            let mut left: Vec<&(u16, XrValue)> = gb.items.iter().collect();
            ga.kind == gb.kind
                && ga.items.iter().all(|item| match left.iter().position(|x| *x == item) {
                    Some(i) => {
                        left.swap_remove(i);
                        true
                    }
                    None => false,
                })
        }
        _ => a == b,
    }
}
