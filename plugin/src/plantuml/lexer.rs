//! Turns raw PlantUML text into logical lines: comments removed, continuation lines
//! joined, `@startuml`/`@enduml` stripped, preprocessor directives rejected. Every line
//! keeps the number of the physical line it started on so errors can point at it.

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub no: usize,
    pub text: String,
}

#[derive(Debug)]
pub struct Source {
    /// The word after `@start`: `uml` for plain diagrams, `gantt`, `mindmap`, ...
    pub tag: String,
    pub tag_line: usize,
    pub lines: Vec<Line>,
    pub warnings: Vec<String>,
}

/// Strips `'` line comments and `/' ... '/` block comments, keeping line numbering.
fn strip_comments(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut in_block = false;
    for (i, raw) in text.lines().enumerate() {
        let mut line = raw.to_string();
        if in_block {
            match line.find("'/") {
                Some(p) => {
                    line = line[p + 2..].to_string();
                    in_block = false;
                }
                None => {
                    out.push((i + 1, String::new()));
                    continue;
                }
            }
        }
        while let Some(p) = line.find("/'") {
            match line[p + 2..].find("'/") {
                Some(q) => line = format!("{}{}", &line[..p], &line[p + 2 + q + 2..]),
                None => {
                    line.truncate(p);
                    in_block = true;
                    break;
                }
            }
        }
        if line.trim_start().starts_with('\'') {
            line.clear();
        }
        out.push((i + 1, line.trim().to_string()));
    }
    out
}

/// A trailing backslash continues the statement on the next line. Arrow tails such as
/// `-\` and the escape `\\` are not continuations.
fn continues(line: &str) -> bool {
    line.ends_with('\\') && !line.ends_with("\\\\") && !line.ends_with("-\\") && !line.ends_with("<\\") && line.len() > 1
}

pub fn lex(source: &[u8]) -> Result<Source, String> {
    let text = std::str::from_utf8(source).map_err(|_| "the PlantUML source is not valid UTF-8".to_string())?;
    let text = text.trim_start_matches('\u{feff}');
    let stripped = strip_comments(text);

    // Only the first @start block counts; anything outside it is ignored when there is one.
    let start = stripped.iter().position(|(_, l)| l.to_ascii_lowercase().starts_with("@start"));
    let (tag, tag_line, body): (String, usize, &[(usize, String)]) = match start {
        Some(i) => {
            let word = stripped[i].1[6..].split_whitespace().next().unwrap_or("uml").to_ascii_lowercase();
            let end = stripped[i + 1..].iter().position(|(_, l)| l.to_ascii_lowercase().starts_with("@end")).map_or(stripped.len(), |p| i + 1 + p);
            (word, stripped[i].0, &stripped[i + 1..end])
        }
        None => ("uml".to_string(), 1, &stripped[..]),
    };

    let mut lines: Vec<Line> = Vec::new();
    let mut warnings = Vec::new();
    let mut i = 0;
    while i < body.len() {
        let (no, mut t) = (body[i].0, body[i].1.clone());
        while continues(&t) && i + 1 < body.len() {
            t.pop();
            let next = &body[i + 1].1;
            let sep = if t.ends_with(' ') || t.is_empty() { "" } else { " " };
            t = format!("{t}{sep}{next}");
            i += 1;
        }
        i += 1;
        if let Some(rest) = t.strip_prefix('!') {
            let word = rest.split(|c: char| c.is_whitespace() || c == '(').next().unwrap_or("").to_ascii_lowercase();
            match word.as_str() {
                "theme" => warnings.push(format!("line {no}: !theme is not supported and is ignored, use skinparam instead")),
                "pragma" => warnings.push(format!("line {no}: !pragma is ignored")),
                _ => return Err(format!("line {no}: the preprocessor directive !{word} is not supported")),
            }
            continue;
        }
        lines.push(Line { no, text: t });
    }
    Ok(Source { tag, tag_line, lines, warnings })
}

pub fn first_word(s: &str) -> &str {
    s.split(|c: char| c.is_whitespace()).next().unwrap_or("")
}

/// Text after the first word, trimmed.
pub fn rest_after_word(s: &str) -> &str {
    s[first_word(s).len()..].trim()
}

/// Index of the first `ch` that is not inside double quotes.
pub fn find_outside_quotes(s: &str, ch: char) -> Option<usize> {
    let mut quoted = false;
    for (i, c) in s.char_indices() {
        if c == '"' {
            quoted = !quoted;
        } else if c == ch && !quoted {
            return Some(i);
        }
    }
    None
}

/// How deep groups, packages and control-flow blocks may nest. The parsers and layouts
/// recurse along the nesting, and wasm has little stack to spare.
pub const MAX_NESTING: usize = 200;

/// An error pointing at line `no`, in the format every parser uses.
pub fn err<T>(no: usize, msg: impl std::fmt::Display) -> Result<T, String> {
    Err(format!("line {no}: {msg}"))
}

/// Errors when opening one more block at line `no` would exceed `MAX_NESTING`.
pub fn check_nesting(open_blocks: usize, no: usize) -> Result<(), String> {
    if open_blocks >= MAX_NESTING {
        return err(no, format_args!("nesting too deep (more than {MAX_NESTING} levels)"));
    }
    Ok(())
}

pub fn unquote(s: &str) -> &str {
    let t = s.trim();
    t.strip_prefix('"').and_then(|x| x.strip_suffix('"')).unwrap_or(t)
}

/// Whether the line opens a block that runs until an explicit end line, returning the
/// predicate for that end line. Used to keep one parser from tripping over lines that
/// belong to another's block (a note body that starts with `title`, say).
pub fn block_end(text: &str) -> Option<fn(&str) -> bool> {
    let lower = text.to_ascii_lowercase();
    let w = first_word(&lower);
    let note_word = |w: &str| matches!(w, "note" | "hnote" | "rnote");
    let single_line = find_outside_quotes(text, ':').is_some();
    if note_word(w) || (w == "floating" && note_word(first_word(rest_after_word(&lower)))) {
        return if single_line { None } else { Some(|l| is_end(l, &["note", "hnote", "rnote"])) };
    }
    if w == "ref" && !single_line {
        return Some(|l| is_end(l, &["ref"]));
    }
    let colored_action = text.starts_with('#') && text[1..].contains(':') && !text.contains(' ');
    if (text.starts_with(':') && !text.ends_with(':') || colored_action) && !ends_action(text) {
        return Some(ends_action);
    }
    None
}

pub fn ends_action(l: &str) -> bool {
    matches!(l.trim_end().chars().last(), Some(';' | '|' | '<' | '>' | '/' | ']' | '}'))
}

/// `end note`, `endnote`, `end hnote` and friends.
pub fn is_end(line: &str, words: &[&str]) -> bool {
    let l = line.trim().to_ascii_lowercase();
    let l = l.replace(' ', "");
    words.iter().any(|w| l == format!("end{w}"))
}

/// Cursor over lines for the diagram parsers.
pub struct Cursor<'a> {
    pub lines: &'a [Line],
    pub pos: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(lines: &'a [Line]) -> Cursor<'a> {
        Cursor { lines, pos: 0 }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<&'a Line> {
        let l = self.lines.get(self.pos)?;
        self.pos += 1;
        Some(l)
    }

    pub fn peek(&self) -> Option<&'a Line> {
        self.lines.get(self.pos)
    }

    /// Lines up to (not including) the one `is_end` accepts; that line is consumed.
    /// `start` is the line number the block began on, for the error.
    pub fn block(&mut self, start: usize, what: &str, is_end: impl Fn(&str) -> bool) -> Result<Vec<&'a Line>, String> {
        let mut out = Vec::new();
        while let Some(l) = self.next() {
            if is_end(&l.text) {
                return Ok(out);
            }
            out.push(l);
        }
        err(start, format_args!("{what} is never closed"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(src: &str) -> Vec<(usize, String)> {
        lex(src.as_bytes()).unwrap().lines.into_iter().map(|l| (l.no, l.text)).collect()
    }

    #[test]
    fn start_end_are_optional() {
        assert_eq!(texts("@startuml foo\nA -> B\n@enduml\ntrailing"), vec![(2, "A -> B".to_string())]);
        assert_eq!(texts("A -> B"), vec![(1, "A -> B".to_string())]);
        assert_eq!(lex(b"@startgantt\n@endgantt").unwrap().tag, "gantt");
    }

    #[test]
    fn comments() {
        let t = texts("' one\nA -> B /' inline '/ : x\n/' block\nstill\n'/\nC -> D\n  ' indented");
        assert_eq!(t, vec![(1, String::new()), (2, "A -> B  : x".into()), (3, String::new()), (4, String::new()), (5, String::new()), (6, "C -> D".into()), (7, String::new())]);
    }

    #[test]
    fn continuation_keeps_first_line_number() {
        let t = texts("A -> B : a \\\n b\nC -> D");
        assert_eq!(t, vec![(1, "A -> B : a b".to_string()), (3, "C -> D".to_string())]);
        assert_eq!(texts("A -\\\nB")[0].1, "A -\\");
    }

    #[test]
    fn directives() {
        let s = lex(b"!theme plain\nA -> B").unwrap();
        assert_eq!(s.warnings.len(), 1);
        assert!(s.warnings[0].starts_with("line 1:"));
        let e = lex(b"A -> B\n!include foo.puml").unwrap_err();
        assert!(e.starts_with("line 2:") && e.contains("!include"), "{e}");
        assert!(lex(b"!define X 1").unwrap_err().contains("!define"));
    }

    #[test]
    fn block_detection() {
        assert!(block_end("note left of A").is_some());
        assert!(block_end("note left of A : x").is_none());
        assert!(block_end(":multi").is_some());
        assert!(block_end(":done;").is_none());
        assert!(block_end("ref over A, B").is_some());
        assert!(is_end("End Note", &["note"]));
    }
}
