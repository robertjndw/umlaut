//! Compiles one document per PlantUML fixture (tests/e2e/plantuml/<name>.typ, which does
//! `plantuml(read("<name>.puml"))`) with the typst CLI and checks the pictures have what
//! the diagrams put in them. Look at the PNGs by eye after layout changes:
//! `typst compile --root . --ppi 144 tests/e2e/plantuml/seq_basic.typ out.png`.
//!
//! Needs `typst` on PATH and a fresh umlaut.wasm (`make wasm`).

mod common;

use std::path::{Path, PathBuf};

fn documents() -> Vec<PathBuf> {
    let dir = common::root().join("tests/e2e/plantuml");
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
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("plantuml");
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

    for (doc, png, child) in children {
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{} failed:\n{}", doc.display(), String::from_utf8_lossy(&output.stderr));
        let img = image::open(&png).unwrap().to_rgba8();
        let name = doc.file_stem().unwrap().to_string_lossy().to_string();
        assert!(img.width() > 40 && img.height() > 40, "{name}: picture is tiny");
        let dark = img.pixels().filter(|p| p.0[0] < 90 && p.0[3] == 255).count();
        assert!(dark > 100, "{name}: nothing dark drawn");
        if name.starts_with("seq_") && name != "seq_style" {
            // Participant boxes use PlantUML's lavender.
            assert!(has_color(&img, [0xe2, 0xe2, 0xf0]), "{name}: participant fill is missing");
        }
        if name == "act_basic" || name == "act_if" {
            assert!(has_color(&img, [0xf1, 0xf1, 0xf1]), "{name}: action fill is missing");
        }
    }
}

#[test]
fn warnings_are_queryable() {
    // The document asserts on the metadata itself, which works the same on every Typst
    // version (`typst query` was reworked in 0.15).
    common::compile_ok("tests/warnings/plantuml.typ");
}

#[test]
fn layout_problems_are_reported() {
    common::compile_ok("tests/warnings/plantuml-layout.typ");
    common::compile_ok("tests/warnings/plantuml-clean.typ");
}

#[test]
fn every_fixture_has_a_document() {
    let fixtures = common::root().join("plugin/tests/fixtures/plantuml");
    for entry in std::fs::read_dir(fixtures).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "puml") {
            let doc = common::root().join("tests/e2e/plantuml").join(path.file_stem().unwrap()).with_extension("typ");
            assert!(doc.exists(), "{} has no document {}", path.display(), doc.display());
        }
    }
}

#[test]
fn multibyte_text_compiles() {
    common::compile_ok("tests/e2e/plantuml-unicode.typ");
}
