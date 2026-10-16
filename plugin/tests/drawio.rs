//! Compiles every tests/e2e/drawio-*.typ (each draws a fixture from
//! plugin/tests/fixtures/drawio through the Typst package) and checks the picture.
//!
//! Needs `typst` on PATH and a fresh umlaut.wasm (`make wasm`). To look at the
//! result: `typst compile --root . --ppi 144 tests/e2e/drawio-flowchart.typ out.png`.

mod common;

use std::path::{Path, PathBuf};

use image::RgbaImage;

fn render(name: &str) -> RgbaImage {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("drawio");
    std::fs::create_dir_all(&dir).unwrap();
    let png: PathBuf = dir.join(format!("{name}.png"));
    let src = format!("tests/e2e/drawio-{name}.typ");
    common::typst_ok(&["compile", "--root", ".", "--ppi", "72", &src, png.to_str().unwrap()]);
    image::open(&png).unwrap().to_rgba8()
}

fn count(img: &RgbaImage, want: [u8; 3]) -> usize {
    img.pixels().filter(|p| (0..3).all(|i| (p.0[i] as i32 - want[i] as i32).abs() <= 2) && p.0[3] == 255).count()
}

fn dark(img: &RgbaImage) -> usize {
    img.pixels().filter(|p| p.0[0] < 90 && p.0[1] < 90 && p.0[2] < 90 && p.0[3] == 255).count()
}

#[test]
fn flowchart_has_its_fills_text_and_lines() {
    let img = render("flowchart");
    assert!(count(&img, [0xd5, 0xe8, 0xd4]) > 1000, "start/end fill missing");
    assert!(count(&img, [0xda, 0xe8, 0xfc]) > 1000, "process fill missing");
    assert!(count(&img, [0xff, 0xf2, 0xcc]) > 1000, "decision fill missing");
    assert!(count(&img, [0xf8, 0xce, 0xcc]) > 1000, "error fill missing");
    assert!(dark(&img) > 800, "no text or lines");
}

#[test]
fn every_fixture_compiles() {
    for name in ["edge-styles", "orthogonal", "markers", "containers", "html-labels", "shapes", "er", "compressed", "bare", "svg"] {
        let img = render(name);
        assert!(dark(&img) > 100, "{name} drew almost nothing");
    }
}

#[test]
fn pages_are_selected_by_index_and_name() {
    let by_name = render("multipage-name");
    let by_index = render("multipage-index");
    // The "Details" page has the yellow rhombus, the third page the red box.
    assert!(count(&by_name, [0xff, 0xf2, 0xcc]) > 500);
    assert!(count(&by_name, [0xf8, 0xce, 0xcc]) == 0);
    assert!(count(&by_index, [0xf8, 0xce, 0xcc]) > 500);
    assert!(count(&by_index, [0xff, 0xf2, 0xcc]) == 0);
}

#[test]
fn unsupported_content_is_reported_as_warnings() {
    common::compile_ok("tests/warnings/drawio.typ");
}

#[test]
fn width_scales_the_whole_diagram() {
    let img = render("width");
    assert!((img.width() as i32 - 200).abs() <= 1, "width {}", img.width());
}

#[test]
fn relative_sizes_resolve_against_the_container() {
    common::compile_ok("tests/e2e/size-relative.typ");
}

#[test]
fn hostile_styles_still_compile() {
    common::compile_ok("tests/e2e/drawio-hostile.typ");
}
