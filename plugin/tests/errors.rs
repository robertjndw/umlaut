//! Every file in tests/errors must fail to compile with an error containing the text
//! from its `// expect:` line.

mod common;

#[test]
fn invalid_input_fails_with_helpful_message() {
    let root = common::root();
    let mut files: Vec<_> = std::fs::read_dir(root.join("tests/errors")).unwrap().map(|e| e.unwrap().path()).collect();
    files.sort();
    assert!(!files.is_empty());

    // Each compile is mostly typst start-up, so run them all at once.
    let children: Vec<_> = files
        .iter()
        .map(|file| {
            common::typst(&["compile", "--root", ".", file.to_str().unwrap(), "-", "--format", "pdf"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .expect(common::TYPST_MISSING)
        })
        .collect();

    let mut failures = Vec::new();
    for (file, child) in files.iter().zip(children) {
        let source = std::fs::read_to_string(file).unwrap();
        let expected = source.lines().next().and_then(|l| l.strip_prefix("// expect: ")).expect("missing // expect: header");
        let out = child.wait_with_output().unwrap();
        // Typst 0.13 escapes the quotes in panic messages, 0.15 doesn't.
        let stderr = String::from_utf8_lossy(&out.stderr).replace("\\\"", "\"");
        if out.status.success() {
            failures.push(format!("{}: compiled successfully", file.display()));
        } else if !stderr.contains(expected) {
            failures.push(format!("{}: expected {expected:?} in:\n{stderr}", file.display()));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
