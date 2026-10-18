//! Mermaid label text to `Run`s.
//!
//! Three flavors, because Mermaid itself treats them differently:
//!
//! - `runs`: node, edge, message and note labels. HTML-ish (`<br>`, `<b>`, `<i>`),
//!   `#name;` entity codes, and markdown strings (`` "`**bold**`" ``).
//! - `plain_runs`: class names and members. Entity codes and `<br>` only, because
//!   `List<int>` must not be read as a tag; `~T~` is Mermaid's spelling of `<T>`.
//!
//! PlantUML's creole is never used: its `//`, `--` and `~` rules would mangle URLs and
//! generics.

use crate::drawio::{decode_entities, html_runs, TextProps};
use crate::plantuml::creole::Base;
use crate::plantuml::lexer::unquote;
use crate::scene::Run;

fn props(base: &Base) -> TextProps {
    TextProps {
        size: base.size,
        color: base.color.clone(),
        bold: base.bold,
        italic: base.italic,
        underline: false,
        strike: false,
        font: base.font.clone(),
        link: None,
        pre: false,
    }
}

fn run(text: &str, base: &Base, bold: bool, italic: bool) -> Run {
    let mut p = props(base);
    p.bold |= bold;
    p.italic |= italic;
    p.run(text.to_string())
}

/// Rewrites Mermaid's `#name;` and `#123;` to the HTML entities `&name;` and `&#123;`.
fn entities_to_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find('#') {
        out.push_str(&rest[..p]);
        let after = &rest[p + 1..];
        let len = after.find(|c: char| !c.is_ascii_alphanumeric()).unwrap_or(after.len());
        if len > 0 && after[len..].starts_with(';') {
            out.push('&');
            if after[..len].bytes().all(|b| b.is_ascii_digit()) {
                out.push('#');
            }
            out.push_str(&after[..len]);
            out.push(';');
            rest = &after[len + 1..];
        } else {
            out.push('#');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

pub fn runs(s: &str, base: &Base) -> Vec<Run> {
    let inner = unquote(s);
    if let Some(md) = inner.strip_prefix('`').and_then(|x| x.strip_suffix('`')) {
        return markdown(md, base);
    }
    html_runs(&entities_to_html(inner), &props(base))
}

/// Text for class names, members and similar: no HTML, entity codes decoded, `<br>` a
/// line break, `~T~` a generic.
pub fn plain_runs(s: &str, base: &Base) -> Vec<Run> {
    let mut out = Vec::new();
    for (i, line) in split_breaks(unquote(s)).into_iter().enumerate() {
        if i > 0 {
            out.push(Run::br());
        }
        let text = decode_entities(&entities_to_html(&generics(line)));
        if !text.is_empty() {
            out.push(run(&text, base, false, false));
        }
    }
    out
}

/// Splits at newlines and `<br>`, `<br/>`, `<br />` (any case).
fn split_breaks(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut start, mut i) = (0, 0);
    while i < s.len() {
        let rest = &s[i..];
        let br = rest.get(..3).filter(|h| h.eq_ignore_ascii_case("<br")).and_then(|_| {
            let end = rest.find('>')?;
            let inner = rest[3..end].trim();
            (inner.is_empty() || inner == "/").then_some(end + 1)
        });
        let skip = match br {
            Some(n) => n,
            None if rest.starts_with('\n') => 1,
            None => {
                i += rest.chars().next().unwrap().len_utf8();
                continue;
            }
        };
        parts.push(&s[start..i]);
        i += skip;
        start = i;
    }
    parts.push(&s[start..]);
    parts
}

/// `List~List~int~~` to `List<List<int>>`. A `~` opens a generic when something that
/// looks like a type name follows and closes one when a generic is open and it doesn't;
/// a lone `~` stays as it is.
fn generics(s: &str) -> String {
    if !s.contains('~') {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut depth = 0;
    for (i, &c) in chars.iter().enumerate() {
        if c != '~' {
            out.push(c);
            continue;
        }
        let starts_name = chars.get(i + 1).is_some_and(|n| n.is_alphanumeric() || *n == '_');
        let later = chars[i + 1..].contains(&'~');
        if starts_name && later {
            depth += 1;
            out.push('<');
        } else if depth > 0 {
            depth -= 1;
            out.push('>');
        } else {
            out.push('~');
        }
    }
    out
}

/// The subset of markdown Mermaid labels use: `**bold**`, `*italic*`, `_italic_`,
/// `***both***` and real newlines as breaks.
fn markdown(s: &str, base: &Base) -> Vec<Run> {
    let mut out = Vec::new();
    for (n, line) in s.lines().enumerate() {
        if n > 0 {
            out.push(Run::br());
        }
        markdown_line(line.trim(), base, &mut out);
    }
    out
}

fn markdown_line(line: &str, base: &Base, out: &mut Vec<Run>) {
    let (mut bold, mut italic) = (false, false);
    let mut buf = String::new();
    let flush = |buf: &mut String, bold: bool, italic: bool, out: &mut Vec<Run>| {
        if !buf.is_empty() {
            out.push(run(&decode_entities(&entities_to_html(buf)), base, bold, italic));
            buf.clear();
        }
    };
    let mut i = 0;
    while i < line.len() {
        let rest = &line[i..];
        let prev = line[..i].chars().last();
        let marker = ["***", "**", "*", "_"].into_iter().find(|m| rest.starts_with(m));
        let usable = marker.filter(|m| {
            let active = match *m {
                "***" => bold && italic,
                "**" => bold,
                _ => italic,
            };
            // Opening needs a closing marker later on, and `_` only opens at a word
            // edge so that snake_case stays intact.
            let edge = *m != "_" || prev.is_none_or(|p| !p.is_alphanumeric());
            active || (edge && rest[m.len()..].contains(m))
        });
        match usable {
            Some(m) => {
                flush(&mut buf, bold, italic, out);
                match m {
                    "***" => {
                        let on = !(bold && italic);
                        bold = on;
                        italic = on;
                    }
                    "**" => bold = !bold,
                    _ => italic = !italic,
                }
                i += m.len();
            }
            None => {
                let c = rest.chars().next().unwrap();
                buf.push(c);
                i += c.len_utf8();
            }
        }
    }
    flush(&mut buf, bold, italic, out);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Base {
        Base::new(16.0, "#333333")
    }

    fn texts(runs: &[Run]) -> Vec<String> {
        runs.iter().map(|r| if r.br { "|".to_string() } else { r.text.clone() }).collect()
    }

    #[test]
    fn plain_and_quoted_labels() {
        assert_eq!(texts(&runs("Hello", &base())), ["Hello"]);
        assert_eq!(texts(&runs("\"Hello there\"", &base())), ["Hello there"]);
        assert_eq!(runs("x", &base())[0].size, 16.0);
    }

    #[test]
    fn html_breaks_and_markup() {
        assert_eq!(texts(&runs("a<br>b<br/>c", &base())), ["a", "|", "b", "|", "c"]);
        let r = runs("a <b>bold</b> and <i>it</i>", &base());
        assert!(r.iter().any(|r| r.bold && r.text == "bold"));
        assert!(r.iter().any(|r| r.italic && r.text == "it"));
        assert_eq!(texts(&runs("a < b", &base())).concat(), "a < b");
    }

    #[test]
    fn newlines_from_joined_strings_break() {
        assert_eq!(texts(&runs("\"one\ntwo\"", &base())), ["one", "|", "two"]);
    }

    #[test]
    fn entity_codes() {
        assert_eq!(texts(&runs("say #quot;hi#quot;", &base())).concat(), "say \"hi\"");
        assert_eq!(texts(&runs("#9829; love", &base())).concat(), "\u{2665} love");
        assert_eq!(texts(&runs("#fff and # 1 and #x", &base())).concat(), "#fff and # 1 and #x");
        assert_eq!(entities_to_html("a#lt;b#59;"), "a&lt;b&#59;");
    }

    #[test]
    fn markdown_strings() {
        let r = runs("\"`**bold** and *it* and ***both***`\"", &base());
        let find = |t: &str| r.iter().find(|r| r.text == t).unwrap().clone();
        assert!(find("bold").bold && !find("bold").italic);
        assert!(find("it").italic && !find("it").bold);
        assert!(find("both").bold && find("both").italic);
        assert!(!find(" and ").bold);
        assert_eq!(texts(&runs("\"`one\ntwo`\"", &base())), ["one", "|", "two"]);
    }

    #[test]
    fn markdown_leaves_unmatched_markers_alone() {
        assert_eq!(texts(&runs("\"`2 * 3 and snake_case_name`\"", &base())).concat(), "2 * 3 and snake_case_name");
        assert_eq!(texts(&runs("\"`_it_ here`\"", &base()))[0], "it");
    }

    #[test]
    fn plain_runs_keep_angle_brackets() {
        assert_eq!(texts(&plain_runs("List<int> items", &base())).concat(), "List<int> items");
        assert_eq!(texts(&plain_runs("a<br>b", &base())), ["a", "|", "b"]);
        assert_eq!(texts(&plain_runs("a #lt;b#gt;", &base())).concat(), "a <b>");
    }

    #[test]
    fn generics_in_plain_runs() {
        let g = |s: &str| texts(&plain_runs(s, &base())).concat();
        assert_eq!(g("List~int~"), "List<int>");
        assert_eq!(g("List~List~int~~"), "List<List<int>>");
        assert_eq!(g("Map~K,V~ x"), "Map<K,V> x");
        assert_eq!(g("~doc"), "~doc");
        assert_eq!(g("+getList() List~int~"), "+getList() List<int>");
    }
}
