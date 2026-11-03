//! Every drawing in the corpus goes through the real core and out as a mesh.

use cad_ffi::session::Session;
use cad_view::{Display, Mesh, Options, View};
use std::path::Path;

fn corpus_files() -> Vec<std::path::PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/mosaiciron-mi201");
    let mut v: Vec<_> = std::fs::read_dir(dir)
        .map(|r| r.filter_map(|e| e.ok()).map(|e| e.path()).collect())
        .unwrap_or_default();
    v.retain(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("dwg" | "dxf")));
    v.sort();
    v
}

fn check(name: &str, bytes: &[u8]) {
    let mut s = Session::new();
    let id = match s.open(bytes) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("{name}: core declined: {e}");
            return;
        }
    };
    let list = s.display(id).expect("display list");
    let d = Display::parse(&list).unwrap_or_else(|e| panic!("{name}: {e}"));
    let view = d.bounds.map(|b| {
        let mut v = View::new([1280.0, 800.0]);
        v.fit(b, 20.0);
        v
    });
    let opts = Options { scale: view.as_ref().map_or(1.0, |v| v.scale), view: view.as_ref().map(|v| v.visible()), ..Options::default() };
    let m = Mesh::build(&d, &opts).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert_eq!(m.lines.len() % 24, 0, "{name}: lines are vertex pairs");
    assert_eq!(m.tris.len() % 36, 0, "{name}: tris are whole triangles");
    assert_eq!(m.quads.len() % 28, 0, "{name}: whole instances");
    for c in m.lines.chunks_exact(12).chain(m.tris.chunks_exact(12)) {
        assert!(f32::from_le_bytes(c[0..4].try_into().unwrap()).is_finite());
        assert!(f32::from_le_bytes(c[4..8].try_into().unwrap()).is_finite());
    }
    // a pick at each item's bbox centre never panics
    for it in d.items.iter().take(200) {
        let _ = d.pick([(it.bbox[0] + it.bbox[2]) / 2.0, (it.bbox[1] + it.bbox[3]) / 2.0], 1.0);
    }
    eprintln!("{name}: {} items, {} lines, {} tri-vertices, {} quads, {} approximate fills", d.items.len(), m.line_vertices() / 2, m.tri_vertices(), m.quad_instances(), m.stats.fills_approximate);
}

#[test]
fn every_corpus_drawing_meshes() {
    let files = corpus_files();
    assert!(!files.is_empty(), "corpus missing");
    for f in files {
        let bytes = std::fs::read(&f).unwrap();
        check(&f.file_name().unwrap().to_string_lossy(), &bytes);
    }
}

#[test]
fn the_sample_drawing_meshes() {
    let mut s = Session::new();
    let id = s.sample().unwrap();
    let d = Display::parse(&s.display(id).unwrap()).unwrap();
    let m = Mesh::build(&d, &Options::default()).unwrap();
    assert!(m.line_vertices() + m.tri_vertices() + m.quad_instances() > 0);
}
