//! `cad_dwg::write::check`: a written DWG read back and compared with the
//! drawing it was written from. The files are written by the DWG crate's
//! own writer (`to_bytes_with`): `cad_core::dwg::write::to_bytes` adds
//! dimension blocks first, so its file holds more than the drawing given.

use cad_core::model::*;
use std::path::Path;

fn corpus(name: &str) -> Vec<u8> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/mosaiciron-mi201").join(name);
    std::fs::read(&p).unwrap()
}

#[test]
fn autocads_drawings_written_as_dwg_read_back_whole() {
    // DXF from every version AutoCAD saves: what the writer keeps comes
    // back the same (DXF-only objects and objects it reports leaving out
    // are not counted against it).
    for v in ["r12", "r2000", "r2004", "r2007", "r2010", "r2013", "r2018"] {
        let d = cad_core::dxf::read_bytes(&corpus(&format!("mi201-{v}.dxf"))).unwrap();
        let out = write(&d);
        let c = cad_core::dwg::write::check(&d, &out).unwrap();
        assert!(c.is_whole(), "{v}: {:?}", c);
    }
}

fn write(d: &Drawing) -> cad_core::dwg::write::DwgOutput {
    cad_core::dwg::write::to_bytes_with(d, &Default::default()).unwrap()
}

fn mi201() -> Drawing {
    cad_core::dxf::read_bytes(&corpus("mi201-r2018.dxf")).unwrap()
}

#[test]
fn a_value_that_reads_back_different_is_found() {
    let d = mi201();
    let out = write(&d);
    // Compare the file with a drawing that differs in one line's end.
    let mut other = d.clone();
    let line = other
        .objects
        .values_mut()
        .find_map(|o| match &mut o.kind {
            ObjectKind::Line(l) => Some(l),
            _ => None,
        })
        .expect("a line");
    line.end.x += 1.0;
    let c = cad_core::dwg::write::check(&other, &out).unwrap();
    assert!(!c.is_whole());
    assert_eq!(c.changed.get("LINE"), Some(&1), "{c:?}");
    assert!(c.vanished.is_empty());
    assert!(c.notes.iter().any(|n| n.contains("LINE: its data")), "{:?}", c.notes);
}

#[test]
fn an_object_missing_from_the_file_is_found() {
    let d = mi201();
    let out = write(&d);
    // A drawing with one more circle than the file holds.
    let mut other = d.clone();
    let mut extra = other.objects.values().find(|o| o.type_name == "CIRCLE" || o.type_name == "LINE").expect("an entity").clone();
    extra.handle = other.new_handle();
    let t = extra.type_name.clone();
    other.objects.insert(extra.handle, extra);
    let c = cad_core::dwg::write::check(&other, &out).unwrap();
    assert_eq!(c.vanished.get(&t), Some(&1), "{c:?}");
    // Objects the writer says it left out are not counted again.
    let mut said = out.clone();
    said.left_out.insert(t.clone(), 1);
    assert!(cad_core::dwg::write::check(&other, &said).unwrap().is_whole());
}

/// The table style's groups in the order another program writes them
/// (each border kind for all six borders, then the next kind), with
/// AutoCAD's default margins.
fn table_style_reordered(d: &mut Drawing) -> Handle {
    let o = d.objects.values_mut().find(|o| o.type_name == "TABLESTYLE").expect("a table style");
    let ObjectKind::NonGraphic(NonGraphic::Groups(g)) = &mut o.kind else { panic!("{:?}", o.kind) };
    // Border groups 274–279, 284–289 and 64–69 of each row: kind by kind.
    let border = |c: u16| (274..=279).contains(&c) || (284..=289).contains(&c) || (64..=69).contains(&c);
    let mut items = Vec::new();
    let mut row: Vec<(u16, XrValue)> = Vec::new();
    for item in g.items.drain(..) {
        if border(item.0) {
            row.push(item);
            continue;
        }
        if !row.is_empty() {
            row.sort_by_key(|(c, _)| match c { 274..=279 => 0, 284..=289 => 1, _ => 2 });
            items.append(&mut row);
        }
        items.push(item);
    }
    row.sort_by_key(|(c, _)| match c { 274..=279 => 0, 284..=289 => 1, _ => 2 });
    items.append(&mut row);
    g.items = items;
    o.handle
}

#[test]
fn groups_in_another_programs_order_read_back_whole() {
    let mut d = mi201();
    let h = table_style_reordered(&mut d);
    let out = write(&d);
    let c = cad_core::dwg::write::check(&d, &out).unwrap();
    assert!(c.is_whole(), "{:X}: {c:?}", h.0);
}
