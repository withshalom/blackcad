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

