//! DXF names, caret-encoded text and duplicate handles, read and saved
//! through DXF and DWG. Each case was found by a program that saves through
//! these crates (DraftCraft); the inputs are hand-written from the DXF
//! reference.

use cad_core::dxf::read_bytes;
use cad_core::model::*;

fn dxf(lines: &[&str]) -> Vec<u8> {
    let mut s = lines.join("\n");
    s.push('\n');
    s.into_bytes()
}

fn through_dwg(d: &Drawing) -> Drawing {
    let dwg = cad_core::dwg::write::to_bytes(d).unwrap().bytes;
    cad_core::open_bytes(&dwg).unwrap()
}

fn layer_name(d: &Drawing, h: Handle) -> String {
    match &d.objects.get(&h).expect("current layer").kind {
        ObjectKind::Layer(l) => l.name.clone(),
        k => panic!("{k:?}"),
    }
}

/// A drawing with the layer `layer` and `$CLAYER` naming `current`.
fn current_layer(layer: &str, current: &str) -> Drawing {
    read_bytes(&dxf(&[
        "0", "SECTION", "2", "HEADER", "9", "$CLAYER", "8", current, "0", "ENDSEC",
        "0", "SECTION", "2", "TABLES",
        "0", "TABLE", "2", "LAYER", "70", "2",
        "0", "LAYER", "2", "0", "70", "0", "62", "7", "6", "Continuous",
        "0", "LAYER", "2", layer, "70", "0", "62", "1", "6", "Continuous",
        "0", "ENDTAB", "0", "ENDSEC",
        "0", "SECTION", "2", "ENTITIES", "0", "ENDSEC", "0", "EOF",
    ]))
    .unwrap()
}

#[test]
fn current_layer_is_found_whatever_its_letter_case() {
    for (layer, current) in [("Survey", "SURVEY"), ("Survey_\\U+00C4", "Survey_\\U+00E4"), ("Survey_Ä", "survey_ä"), ("Survey_\\U+00C4", "SURVEY_Ä")] {
        let d = current_layer(layer, current);
        assert_eq!(layer_name(&d, d.header.clayer).to_uppercase(), layer.to_uppercase(), "{layer} / {current}");
        let back = through_dwg(&d);
        assert_ne!(layer_name(&back, back.header.clayer), "0", "DWG: {layer} / {current}");
    }
}

fn mtext_contents(d: &Drawing) -> Vec<String> {
    d.model_space_entities()
        .iter()
        .filter_map(|o| match &o.kind {
            ObjectKind::MText(m) => Some(m.contents.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn caret_encoded_text_reads_as_the_text_it_stands_for() {
    // "^ " is a caret; "^J" a line feed; "^I" a tab.
    let d = read_bytes(&dxf(&[
        "0", "SECTION", "2", "ENTITIES",
        "0", "MTEXT", "8", "0", "10", "0", "20", "0", "30", "0", "40", "1", "1", "^  C",
        "0", "MTEXT", "8", "0", "10", "0", "20", "0", "30", "0", "40", "1", "1", "a^Jb^Ic",
        "0", "MTEXT", "8", "0", "10", "0", "20", "0", "30", "0", "40", "1", "1", "x^",
        "0", "ENDSEC", "0", "EOF",
    ]))
    .unwrap();
    let want = ["^ C", "a\nb\tc", "x^"];
    assert_eq!(mtext_contents(&d), want);
    assert_eq!(mtext_contents(&through_dwg(&d)), want, "DWG keeps the text, not its DXF encoding");
    // Written again, it reads back the same.
    let again = read_bytes(&cad_core::dxf::to_bytes(&d, cad_core::dxf::DxfFormat::Ascii).bytes).unwrap();
    assert_eq!(mtext_contents(&again), want);
}

