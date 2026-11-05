//! A block's "explodable" setting, read and saved through DXF and DWG.
//! Found by a program that saves through these crates (DraftCraft); the
//! input is hand-written from the DXF reference.

use cad_core::dxf::read_bytes;
use cad_core::model::*;

fn with_block(explodable: &str) -> Drawing {
    let lines = [
        "0", "SECTION", "2", "TABLES",
        "0", "TABLE", "2", "BLOCK_RECORD", "5", "1", "70", "3",
        "0", "BLOCK_RECORD", "5", "1F", "100", "AcDbSymbolTableRecord", "100", "AcDbBlockTableRecord", "2", "*Model_Space",
        "0", "BLOCK_RECORD", "5", "1B", "100", "AcDbSymbolTableRecord", "100", "AcDbBlockTableRecord", "2", "*Paper_Space",
        "0", "BLOCK_RECORD", "5", "30", "100", "AcDbSymbolTableRecord", "100", "AcDbBlockTableRecord", "2", "LEAF", "70", "4", "280", explodable,
        "0", "ENDTAB", "0", "ENDSEC",
        "0", "SECTION", "2", "BLOCKS",
        "0", "BLOCK", "5", "31", "330", "30", "100", "AcDbEntity", "8", "0", "100", "AcDbBlockBegin", "2", "LEAF", "70", "0", "10", "0", "20", "0", "30", "0", "3", "LEAF",
        "0", "LINE", "5", "32", "330", "30", "100", "AcDbEntity", "8", "0", "100", "AcDbLine", "10", "0", "20", "0", "30", "0", "11", "1", "21", "1", "31", "0",
        "0", "ENDBLK", "5", "33", "330", "30", "100", "AcDbEntity", "8", "0", "100", "AcDbBlockEnd",
        "0", "ENDSEC",
        "0", "SECTION", "2", "ENTITIES", "0", "ENDSEC", "0", "EOF",
    ];
    let mut s = lines.join("\n");
    s.push('\n');
    read_bytes(s.as_bytes()).unwrap()
}

fn leaf(d: &Drawing) -> BlockHeader {
    d.objects
        .values()
        .find_map(|o| match &o.kind {
            ObjectKind::BlockHeader(b) if b.name == "LEAF" => Some(b.clone()),
            _ => None,
        })
        .expect("block LEAF")
}

#[test]
fn a_block_that_may_not_be_exploded_stays_so() {
    for (value, not_explodable) in [("0", true), ("1", false)] {
        let d = with_block(value);
        assert_eq!(leaf(&d).not_explodable, not_explodable, "DXF 280={value}");
        assert_eq!(leaf(&d).insert_units, 4);
        let dwg = cad_core::dwg::write::to_bytes(&d).unwrap().bytes;
        let back = cad_core::open_bytes(&dwg).unwrap();
        assert_eq!(leaf(&back).not_explodable, not_explodable, "DWG, 280={value}");
        let dxf = cad_core::dxf::to_bytes(&back, cad_core::dxf::DxfFormat::Ascii).bytes;
        assert_eq!(leaf(&read_bytes(&dxf).unwrap()).not_explodable, not_explodable, "DXF again, 280={value}");
    }
}
