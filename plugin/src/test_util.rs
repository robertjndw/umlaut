//! Helpers shared by the front ends' test suites. Text is "measured" with a made-up font
//! (every character 0.55 em wide, lines 1.2 em high), which is enough to check layout
//! invariants without Typst.

use std::path::PathBuf;

use crate::scene::{Label, Run, Scene};

/// `(name, contents)` of every `tests/fixtures/<dir>/*.<ext>` whose name starts with
/// `prefix`, sorted by name. Panics when nothing matches, so a typo cannot make a
/// test pass by looping over nothing.
pub(crate) fn fixtures(dir: &str, ext: &str, prefix: &str) -> Vec<(String, String)> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(dir);
    let mut out: Vec<_> = std::fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == ext))
        .map(|p| (p.file_stem().unwrap().to_string_lossy().to_string(), std::fs::read_to_string(&p).unwrap()))
        .filter(|(n, _)| n.starts_with(prefix))
        .collect();
    out.sort();
    assert!(!out.is_empty(), "no fixtures with prefix {prefix}");
    out
}

pub(crate) fn fake_size(l: &Label) -> (f32, f32) {
    let mut lines: Vec<(f32, f32)> = vec![(0.0, 0.0)];
    for r in &l.runs {
        let cur = lines.last_mut().unwrap();
        if r.br {
            lines.push((0.0, 0.0));
            continue;
        }
        cur.0 += r.text.chars().count() as f32 * r.size * 0.55;
        cur.1 = cur.1.max(r.size * 1.2);
    }
    let natural = lines.iter().map(|l| l.0).fold(0.0, f32::max);
    let line_h = lines.iter().map(|l| l.1).fold(0.0, f32::max).max(l.runs.first().map_or(12.0, |r: &Run| r.size * 1.2));
    match l.wrap_width {
        Some(w) => (w, line_h * (lines.len() as f32).max((natural / w).ceil())),
        None => (natural, line_h * lines.len() as f32),
    }
}

pub(crate) fn sizes(labels: &[Label]) -> Vec<(f32, f32)> {
    labels.iter().map(fake_size).collect()
}

/// Runs a front end's two passes with the fake font, the way `_two-pass` in lib.typ does.
pub(crate) fn render_with(
    labels: impl Fn(&[u8]) -> Result<Vec<Label>, String>,
    layout: impl Fn(&[u8], &[(f32, f32)]) -> Result<Scene, String>,
    src: &str,
) -> Result<Scene, String> {
    let l = labels(src.as_bytes())?;
    layout(src.as_bytes(), &sizes(&l))
}
