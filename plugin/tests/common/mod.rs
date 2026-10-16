//! Helpers shared by the integration tests. Each test crate compiles this module on
//! its own and uses only part of it.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// Repository root, where the typst CLI has to run so `--root .` covers the package.
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

pub fn typst(args: &[&str]) -> Command {
    let mut cmd = Command::new("typst");
    cmd.args(args).current_dir(root());
    cmd
}

pub const TYPST_MISSING: &str = "the typst CLI must be on PATH";

/// Runs typst and returns stdout, failing the test with typst's message on error.
pub fn typst_ok(args: &[&str]) -> Vec<u8> {
    let out = typst(args).output().expect(TYPST_MISSING);
    assert!(out.status.success(), "typst {args:?} failed:\n{}", String::from_utf8_lossy(&out.stderr));
    out.stdout
}

/// Compiles a document that checks things itself with `assert`, and fails the test with
/// typst's message when an assertion (or anything else) fails. The output is discarded.
pub fn compile_ok(doc: &str) {
    typst_ok(&["compile", "--root", ".", doc, "-", "--format", "pdf"]);
}
