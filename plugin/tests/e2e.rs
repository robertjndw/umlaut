//! Compiles tests/e2e/scene.typ, which draws the plugin's hand-written fixture scene,
//! and checks the picture has what the fixture puts in it. The fixture is also worth
//! a look by eye after renderer changes: `typst compile --root . tests/e2e/scene.typ out.png`.
//!
//! Needs `typst` on PATH and a fresh umlaut.wasm (`make wasm`).

mod common;

use std::path::Path;

#[test]
fn fixture_scene_renders() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("e2e");
    std::fs::create_dir_all(&dir).unwrap();
    let png = dir.join("scene.png");
    common::typst_ok(&["compile", "--root", ".", "--ppi", "100", "tests/e2e/scene.typ", png.to_str().unwrap()]);

    let img = image::open(&png).unwrap().to_rgba8();
    let has = |want: [u8; 3]| {
        img.pixels().any(|p| {
            let c = p.0;
            (0..3).all(|i| (c[i] as i32 - want[i] as i32).abs() <= 2) && c[3] == 255
        })
    };
    // Fills of the rounded rect (#dae8fc), the triangle (#d5e8d4) and the path (#f8cecc).
    assert!(has([0xda, 0xe8, 0xfc]), "rounded rect fill is missing");
    assert!(has([0xd5, 0xe8, 0xd4]), "polygon fill is missing");
    assert!(has([0xf8, 0xce, 0xcc]), "path fill is missing");
    // Dark pixels come from text and strokes.
    assert!(img.pixels().filter(|p| p.0[0] < 60 && p.0[3] == 255).count() > 200, "no text or strokes drawn");
}

#[test]
fn warnings_are_queryable() {
    common::compile_ok("tests/warnings/scene.typ");
}
