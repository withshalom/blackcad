//! Values that must come back unchanged from a DXF read, a DWG save and a
//! DWG read. Each case was found by a program that saves through these
//! crates (DraftCraft) and compares the drawing it reopens with the one it
//! saved; the inputs here are hand-written from the DXF reference.

use cad_core::dxf::read_bytes;
use cad_core::model::*;

fn dxf(lines: &[&str]) -> Vec<u8> {
    let mut s = lines.join("\n");
    s.push('\n');
    s.into_bytes()
}

/// The drawing saved as DWG and read back.
fn through_dwg(d: &Drawing) -> Drawing {
    let dwg = cad_core::dwg::write::to_bytes(d).unwrap().bytes;
    cad_core::open_bytes(&dwg).unwrap()
}

fn only<'a>(d: &'a Drawing, type_name: &str) -> &'a Object {
    let found: Vec<&Object> = d.model_space_entities().into_iter().filter(|o| o.type_name == type_name).collect();
    assert_eq!(found.len(), 1, "one {type_name}");
    found[0]
}

#[test]
fn mtext_reference_rectangle_keeps_width_and_height() {
    let d = read_bytes(&dxf(&[
        "0", "SECTION", "2", "ENTITIES",
        "0", "MTEXT", "8", "0", "10", "1.0", "20", "2.0", "30", "0.0",
        "40", "2.5", "41", "12.5", "46", "3.0", "71", "1", "72", "5", "1", "first\\Psecond",
        "0", "ENDSEC", "0", "EOF",
    ]))
    .unwrap();
    for (pass, d) in [("DXF", d.clone()), ("DWG", through_dwg(&d))] {
        match &only(&d, "MTEXT").kind {
            ObjectKind::MText(m) => {
                assert_eq!(m.rect_width, 12.5, "{pass}: width (41)");
                assert_eq!(m.rect_height, 3.0, "{pass}: height (46)");
                assert_eq!(m.text_height, 2.5, "{pass}: text height (40)");
            }
            k => panic!("{pass}: {k:?}"),
        }
    }
}

/// A block "B" holding one line, and the `inserts` in model space.
fn with_block(inserts: &[&str]) -> Vec<u8> {
    let mut l: Vec<&str> = vec![
        "0", "SECTION", "2", "BLOCKS",
        "0", "BLOCK", "8", "0", "2", "B", "70", "0", "10", "0.0", "20", "0.0", "30", "0.0", "3", "B",
        "0", "LINE", "8", "0", "10", "0.0", "20", "0.0", "30", "0.0", "11", "1.0", "21", "1.0", "31", "0.0",
        "0", "ENDBLK", "8", "0",
        "0", "ENDSEC",
        "0", "SECTION", "2", "ENTITIES",
    ];
    l.extend_from_slice(inserts);
    l.extend_from_slice(&["0", "ENDSEC", "0", "EOF"]);
    dxf(&l)
}

fn array_of(o: &Object) -> Option<(u16, u16, f64, f64)> {
    match &o.kind {
        ObjectKind::Insert(i) => i.array,
        k => panic!("{k:?}"),
    }
}

#[test]
fn insert_array_without_the_minsert_marker_is_kept() {
    // Counts and spacings with the block reference's own groups, as the
    // DXF reference lists them for INSERT.
    let d = read_bytes(&with_block(&[
        "0", "INSERT", "5", "40", "100", "AcDbEntity", "8", "0", "100", "AcDbBlockReference", "2", "B",
        "10", "1.0", "20", "2.0", "30", "0.0", "70", "1", "71", "3", "44", "5.25", "45", "-3.75",
    ]))
    .unwrap();
    let want = Some((1, 3, 5.25, -3.75));
    assert_eq!(array_of(only(&d, "MINSERT")), want, "DXF");
    let back = through_dwg(&d);
    assert_eq!(array_of(only(&back, "MINSERT")), want, "DWG");
}

#[test]
fn insert_array_in_r12_groups_is_kept() {
    let d = read_bytes(&with_block(&[
        "0", "INSERT", "8", "0", "2", "B", "10", "0.0", "20", "0.0", "30", "0.0", "70", "2", "71", "2", "44", "1.0", "45", "1.0",
    ]))
    .unwrap();
    assert_eq!(array_of(only(&d, "MINSERT")), Some((2, 2, 1.0, 1.0)));
}

#[test]
fn single_insert_is_not_an_array() {
    let d = read_bytes(&with_block(&[
        "0", "INSERT", "5", "40", "100", "AcDbEntity", "8", "0", "100", "AcDbBlockReference", "2", "B",
        "10", "1.0", "20", "2.0", "30", "0.0", "70", "1", "71", "1",
    ]))
    .unwrap();
    assert_eq!(array_of(only(&d, "INSERT")), None);
}

fn one_mtext(groups: &[&str]) -> Drawing {
    let mut l: Vec<&str> = vec!["0", "SECTION", "2", "ENTITIES", "0", "MTEXT", "8", "0", "10", "0.0", "20", "0.0", "30", "0.0", "40", "2.5", "1", "text"];
    l.extend_from_slice(groups);
    l.extend_from_slice(&["0", "ENDSEC", "0", "EOF"]);
    read_bytes(&dxf(&l)).unwrap()
}

fn mtext_of(d: &Drawing) -> MText {
    match &only(d, "MTEXT").kind {
        ObjectKind::MText(m) => m.clone(),
        k => panic!("{k:?}"),
    }
}

#[test]
fn mtext_rotation_without_an_x_axis_sets_the_direction() {
    let d = one_mtext(&["50", "90.0"]);
    for (pass, d) in [("DXF", d.clone()), ("DWG", through_dwg(&d))] {
        let x = mtext_of(&d).x_axis;
        assert!(x.x.abs() < 1e-12 && (x.y - 1.0).abs() < 1e-12, "{pass}: {x:?}");
    }
    // The x-axis wins when both are given.
    let d = one_mtext(&["11", "1.0", "21", "0.0", "31", "0.0", "50", "90.0"]);
    assert_eq!(mtext_of(&d).x_axis, cad_core::geom::Vec3::X);
}

#[test]
fn mtext_line_spacing_style_is_kept() {
    for (style, want) in [("2", true), ("1", false)] {
        let d = one_mtext(&["73", style, "44", "0.25"]);
        for (pass, d) in [("DXF", d.clone()), ("DWG", through_dwg(&d))] {
            let m = mtext_of(&d);
            assert_eq!(m.linespacing_exact, want, "{pass}");
            assert_eq!(m.linespacing_factor, 0.25, "{pass}");
        }
    }
    // Missing: "at least".
    assert!(!mtext_of(&one_mtext(&[])).linespacing_exact);
}
