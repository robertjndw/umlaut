//! Compiles one document per Mermaid fixture (tests/e2e/mermaid/<name>.typ, which does
//! `mermaid(read("<name>.mmd"))`) with the typst CLI and checks the pictures have what
//! the diagrams put in them. Look at the PNGs by eye after layout changes:
//! `typst compile --root . --ppi 144 tests/e2e/mermaid/flow_basic_td.typ out.png`.
//!
//! Needs `typst` on PATH and a fresh umlaut.wasm (`make wasm`).

mod common;

use std::path::{Path, PathBuf};

fn documents() -> Vec<PathBuf> {
    let dir = common::root().join("tests/e2e/mermaid");
    let mut files: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|e| e == "typ")).collect();
    files.sort();
    assert!(!files.is_empty());
    files
}

fn has_color(img: &image::RgbaImage, want: [u8; 3]) -> bool {
    img.pixels().any(|p| (0..3).all(|i| (p.0[i] as i32 - want[i] as i32).abs() <= 2) && p.0[3] == 255)
}

#[test]
fn every_fixture_compiles_and_draws_something() {
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("mermaid");
    std::fs::create_dir_all(&out).unwrap();
    let docs = documents();

    // Each compile is mostly typst start-up, so run them all at once.
    let children: Vec<_> = docs
        .iter()
        .map(|doc| {
            let png = out.join(doc.file_stem().unwrap()).with_extension("png");
            let child = common::typst(&["compile", "--root", ".", "--ppi", "60", doc.to_str().unwrap(), png.to_str().unwrap()])
                .stderr(std::process::Stdio::piped())
                .spawn()
                .expect(common::TYPST_MISSING);
            (doc, png, child)
        })
        .collect();

    // Fixtures that restyle every node, so the theme fill never shows.
    let restyled = ["cls_styled", "st_styled"];

    for (doc, png, child) in children {
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{} failed:\n{}", doc.display(), String::from_utf8_lossy(&output.stderr));
        let img = image::open(&png).unwrap().to_rgba8();
        let name = doc.file_stem().unwrap().to_string_lossy().to_string();
        assert!(img.width() > 40 && img.height() > 40, "{name}: picture is tiny");
        let dark = img.pixels().filter(|p| p.0[0] < 90 && p.0[3] == 255).count();
        assert!(dark > 100, "{name}: nothing dark drawn");
        let themed = ["flow_", "seq_", "cls_", "st_"].iter().any(|p| name.starts_with(p));
        if themed && !restyled.contains(&name.as_str()) {
            // Mermaid's default theme fills nodes, participants and classes with lavender.
            assert!(has_color(&img, [0xec, 0xec, 0xff]), "{name}: theme fill is missing");
        }
    }
}

#[test]
fn warnings_are_queryable() {
    common::compile_ok("tests/warnings/mermaid.typ");
}

#[test]
fn clean_diagrams_have_no_warnings() {
    common::compile_ok("tests/warnings/mermaid-clean.typ");
}

#[test]
fn every_fixture_has_a_document() {
    let fixtures = common::root().join("plugin/tests/fixtures/mermaid");
    for entry in std::fs::read_dir(fixtures).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "mmd") {
            let doc = common::root().join("tests/e2e/mermaid").join(path.file_stem().unwrap()).with_extension("typ");
            assert!(doc.exists(), "{} has no document {}", path.display(), doc.display());
        }
    }
}

#[test]
fn fenced_blocks_render_through_a_show_rule() {
    common::compile_ok("tests/e2e/mermaid-raw.typ");
}
