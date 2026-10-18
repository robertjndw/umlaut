//! Turns raw Mermaid text into logical lines: front matter, directives, comments and
//! accessibility statements removed, multi-line strings joined, `;` split into
//! statements, and the diagram type read off the header. Every line keeps the number of
//! the physical line it started on so errors can point at it.

use crate::plantuml::lexer::{err, Line};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Flowchart,
    Sequence,
    Class,
    State,
}

/// Flowchart direction; `TD` is the same as `TB`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    TB,
    BT,
    LR,
    RL,
}

impl Dir {
    pub fn lr(self) -> bool {
        matches!(self, Dir::LR | Dir::RL)
    }

    /// Ranks run bottom to top or right to left.
    pub fn reverse(self) -> bool {
        matches!(self, Dir::BT | Dir::RL)
    }

    pub fn parse(s: &str) -> Option<Dir> {
        match s {
            "TB" | "TD" | "v" => Some(Dir::TB),
            "BT" | "^" => Some(Dir::BT),
            "LR" | ">" => Some(Dir::LR),
            "RL" | "<" => Some(Dir::RL),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    pub kind: Kind,
    /// Only flowcharts have one on the header line.
    pub dir: Option<Dir>,
    pub line: usize,
}

#[derive(Debug)]
pub struct Source {
    pub header: Header,
    /// The statements after the header.
    pub lines: Vec<Line>,
    /// `title:` of the front matter.
    pub title: Option<String>,
    /// An `%%{init}%%` directive asked for `wrap: true` (sequence diagrams wrap messages).
    pub wrap: bool,
    pub warnings: Vec<String>,
}

const SUPPORTED: &str = "flowchart, sequenceDiagram, classDiagram, stateDiagram";

/// Diagram types Mermaid has that we do not draw.
const UNSUPPORTED: &[&str] = &[
    "gantt",
    "pie",
    "erDiagram",
    "journey",
    "gitGraph",
    "mindmap",
    "timeline",
    "quadrantChart",
    "requirementDiagram",
    "C4Context",
    "C4Container",
    "C4Component",
    "C4Dynamic",
    "C4Deployment",
    "sankey",
    "xychart",
    "block",
    "packet",
    "kanban",
    "architecture",
    "radar",
    "treemap",
];

struct Prelim {
    lines: Vec<(usize, String)>,
    title: Option<String>,
    wrap: bool,
    warnings: Vec<String>,
}

fn unquote(s: &str) -> &str {
    let s = s.trim();
    for q in ['"', '\''] {
        if let Some(inner) = s.strip_prefix(q).and_then(|x| x.strip_suffix(q)) {
            return inner;
        }
    }
    s
}

/// Whether `rest` is `name` followed by a colon, the shape of `accTitle: ...`.
fn acc_statement<'a>(t: &'a str, name: &str) -> Option<&'a str> {
    let rest = t.strip_prefix(name)?.trim_start();
    if name == "accDescr" && rest.starts_with('{') {
        return Some(rest);
    }
    rest.strip_prefix(':')
}

/// Everything that is not a statement: front matter, directives, comments, blank lines
/// and accessibility texts. Lines come back trimmed.
fn prelim(text: &str) -> Result<Prelim, String> {
    let raw: Vec<(usize, &str)> = text.lines().enumerate().map(|(i, l)| (i + 1, l)).collect();
    let mut out = Prelim { lines: Vec::new(), title: None, wrap: false, warnings: Vec::new() };
    let mut pos = 0;

    if let Some(&(open, _)) = raw.iter().find(|(_, l)| !l.trim().is_empty()).filter(|(_, l)| l.trim() == "---") {
        let mut closed = false;
        let mut warned = false;
        pos = open;
        while pos < raw.len() {
            let (no, l) = raw[pos];
            pos += 1;
            let t = l.trim();
            if t == "---" {
                closed = true;
                break;
            }
            if t.is_empty() || t.starts_with('#') {
                continue;
            }
            let top_level = !l.starts_with(char::is_whitespace);
            match t.strip_prefix("title:").filter(|_| top_level) {
                Some(v) => out.title = Some(unquote(v).to_string()),
                None if !warned => {
                    warned = true;
                    out.warnings.push(format!("line {no}: front matter config is ignored"));
                }
                None => {}
            }
        }
        if !closed {
            return err(open, "the front matter opened here with --- is never closed");
        }
    }

    let mut directive: Option<(usize, String)> = None;
    let mut in_descr = false;
    for &(no, l) in &raw[pos..] {
        let t = l.trim();
        if in_descr {
            in_descr = !t.contains('}');
            continue;
        }
        if let Some((_, body)) = directive.as_mut() {
            body.push_str(t);
            if t.contains("}%%") {
                let (_, body) = directive.take().unwrap();
                out.wrap |= asks_for_wrap(&body);
            }
            continue;
        }
        if t.is_empty() {
            continue;
        }
        if t.starts_with("%%{") {
            out.warnings.push(format!("line {no}: %%{{init}}%% directives are ignored"));
            if t.contains("}%%") {
                out.wrap |= asks_for_wrap(t);
            } else {
                directive = Some((no, t.to_string()));
            }
            continue;
        }
        if t.starts_with("%%") || acc_statement(t, "accTitle").is_some() {
            continue;
        }
        if let Some(rest) = acc_statement(t, "accDescr") {
            in_descr = rest.starts_with('{') && !rest.contains('}');
            continue;
        }
        out.lines.push((no, t.to_string()));
    }
    if let Some((no, _)) = directive {
        return err(no, "this %%{ directive is never closed");
    }
    Ok(out)
}

fn asks_for_wrap(directive: &str) -> bool {
    let squeezed: String = directive.chars().filter(|c| !c.is_whitespace() && *c != '"' && *c != '\'').collect();
    squeezed.contains("wrap:true")
}

/// Walks `s` and tells for every character whether it is inside double quotes. Past the
/// first unquoted `:` the rest of a sequence, state or class statement is free text
/// (a message, a description), so an odd quote there must not swallow later lines.
fn quote_state(s: &str, kind: Kind) -> impl Iterator<Item = (usize, char, bool)> + '_ {
    let mut quoted = false;
    let mut free_text = false;
    s.char_indices().map(move |(i, c)| {
        let inside = quoted;
        if !free_text {
            if c == '"' {
                quoted = !quoted;
            } else if c == ':' && !quoted && kind != Kind::Flowchart {
                free_text = true;
            }
        }
        (i, c, inside || (c == '"' && quoted))
    })
}

fn quote_open(s: &str, kind: Kind) -> bool {
    let mut open = false;
    let mut free_text = false;
    for c in s.chars() {
        if free_text {
            break;
        }
        match c {
            '"' => open = !open,
            ':' if !open && kind != Kind::Flowchart => free_text = true,
            _ => {}
        }
    }
    open
}

/// Whether `s` ends in the start of an entity code, `#quot` or `#59`.
fn ends_in_entity(s: &str) -> bool {
    let tail = s.trim_end_matches(|c: char| c.is_ascii_alphanumeric());
    tail.len() < s.len() && tail.ends_with('#')
}

fn split_statements(s: &str, kind: Kind) -> Vec<String> {
    if kind == Kind::Class {
        let t = s.trim();
        let t = match t.strip_suffix(';') {
            Some(head) if !ends_in_entity(head) => head.trim_end(),
            _ => t,
        };
        return if t.is_empty() { Vec::new() } else { vec![t.to_string()] };
    }
    let mut out = Vec::new();
    let mut start = 0;
    for (i, c, quoted) in quote_state(s, kind) {
        if c == ';' && !quoted && !ends_in_entity(&s[start..i]) {
            out.push(s[start..i].trim().to_string());
            start = i + 1;
        }
    }
    out.push(s[start..].trim().to_string());
    out.retain(|p| !p.is_empty());
    out
}

/// The word that names the diagram: up to whitespace or `;`.
fn split_word(s: &str) -> (&str, &str) {
    let end = s.find(|c: char| c.is_whitespace() || c == ';').unwrap_or(s.len());
    (&s[..end], &s[end..])
}

fn parse_header(no: usize, text: &str, warnings: &mut Vec<String>) -> Result<(Header, String), String> {
    if text.starts_with("@start") {
        return err(no, "this is PlantUML, use plantuml() instead of mermaid()");
    }
    let (word, rest) = split_word(text);
    let kind = match word {
        "flowchart" | "graph" => Kind::Flowchart,
        "flowchart-elk" => {
            warnings.push(format!("line {no}: flowchart-elk is drawn with the default layout"));
            Kind::Flowchart
        }
        "sequenceDiagram" => Kind::Sequence,
        "classDiagram" | "classDiagram-v2" => Kind::Class,
        "stateDiagram" | "stateDiagram-v2" => Kind::State,
        other => {
            let name = other.strip_suffix("-beta").unwrap_or(other);
            return if UNSUPPORTED.contains(&name) {
                err(no, format_args!("Mermaid {other} diagrams are not supported yet (supported: {SUPPORTED})"))
            } else {
                err(no, format_args!("expected a diagram type such as `flowchart TD`, found `{other}`"))
            };
        }
    };
    let mut rest = rest.trim_start();
    let mut dir = None;
    if kind == Kind::Flowchart {
        let (tok, after) = split_word(rest);
        if !tok.is_empty() {
            dir = Some(Dir::parse(tok).ok_or_else(|| format!("line {no}: unknown flowchart direction `{tok}`, use TB, TD, BT, LR or RL"))?);
            rest = after;
        }
    }
    let rest = rest.trim_start().trim_start_matches(';');
    Ok((Header { kind, dir, line: no }, rest.to_string()))
}

pub fn lex(source: &[u8]) -> Result<Source, String> {
    let text = std::str::from_utf8(source).map_err(|_| "the Mermaid source is not valid UTF-8".to_string())?;
    let text = text.trim_start_matches('\u{feff}');
    let Prelim { lines, title, wrap, mut warnings } = prelim(text)?;
    let mut it = lines.into_iter();
    let Some((no, first)) = it.next() else {
        return Err("the Mermaid source is empty, expected a diagram type such as `flowchart TD`".to_string());
    };
    let (header, rest) = parse_header(no, &first, &mut warnings)?;
    let kind = header.kind;

    let mut pending: Vec<(usize, String)> = vec![(no, rest)];
    pending.extend(it);
    let mut out = Vec::new();
    let mut i = 0;
    while i < pending.len() {
        let (no, mut t) = pending[i].clone();
        let mut j = i;
        while quote_open(&t, kind) && j + 1 < pending.len() {
            j += 1;
            t.push('\n');
            t.push_str(&pending[j].1);
        }
        // A quote that never closes is literal text, not the rest of the file.
        let joined = !quote_open(&t, kind);
        if !joined {
            t = pending[i].1.clone();
            j = i;
        }
        i = j + 1;
        out.extend(split_statements(&t, kind).into_iter().map(|text| Line { no, text }));
    }
    Ok(Source { header, lines: out, title, wrap, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(src: &str) -> Vec<(usize, String)> {
        lex(src.as_bytes()).unwrap().lines.into_iter().map(|l| (l.no, l.text)).collect()
    }

    fn fails(src: &str) -> String {
        lex(src.as_bytes()).unwrap_err()
    }

    #[test]
    fn utf8_and_bom() {
        assert!(lex(&[0xff, 0xfe]).unwrap_err().contains("not valid UTF-8"));
        let s = lex("\u{feff}flowchart LR\nA-->B".as_bytes()).unwrap();
        assert_eq!(s.header.dir, Some(Dir::LR));
    }

    #[test]
    fn front_matter_title() {
        let s = lex(b"---\ntitle: \"Hello: world\"\n---\nflowchart TD\nA-->B").unwrap();
        assert_eq!(s.title.as_deref(), Some("Hello: world"));
        assert_eq!(s.header.line, 4);
        assert!(s.warnings.is_empty());
        assert_eq!(lex(b"---\ntitle: Plain title\n---\nsequenceDiagram").unwrap().title.as_deref(), Some("Plain title"));
    }

    #[test]
    fn front_matter_config_warns_once() {
        let s = lex(b"---\nconfig:\n  theme: dark\n  look: neo\n---\nflowchart TD").unwrap();
        assert_eq!(s.warnings, vec!["line 2: front matter config is ignored".to_string()]);
    }

    #[test]
    fn unclosed_front_matter_names_the_opening_line() {
        assert!(fails("\n---\ntitle: x\nflowchart TD").starts_with("line 2:"));
        assert!(fails("---\ntitle: x\nflowchart TD").contains("never closed"));
    }

    #[test]
    fn directives_are_ignored_with_a_warning() {
        let s = lex(b"%%{init: {\"theme\": \"dark\"}}%%\nflowchart TD\nA-->B").unwrap();
        assert_eq!(s.warnings, vec!["line 1: %%{init}%% directives are ignored".to_string()]);
        assert!(!s.wrap);
        let s = lex(b"%%{\n  init: {\n    \"sequence\": { \"wrap\": true }\n  }\n}%%\nsequenceDiagram").unwrap();
        assert!(s.wrap);
        assert_eq!(s.header.line, 6);
        assert_eq!(s.warnings.len(), 1);
        assert!(fails("%%{init: {\nflowchart TD").starts_with("line 1:"));
    }

    #[test]
    fn full_line_comments_only() {
        let t = texts("flowchart TD\n%% a comment\nA[100%% sure]-->B\n   %% indented");
        assert_eq!(t, vec![(3, "A[100%% sure]-->B".to_string())]);
    }

    #[test]
    fn headers() {
        let h = |s: &str| lex(s.as_bytes()).unwrap().header;
        assert_eq!(h("graph"), Header { kind: Kind::Flowchart, dir: None, line: 1 });
        assert_eq!(h("graph TD;").dir, Some(Dir::TB));
        assert_eq!(h("flowchart BT").dir, Some(Dir::BT));
        assert_eq!(h("flowchart RL").dir, Some(Dir::RL));
        assert_eq!(h("sequenceDiagram").kind, Kind::Sequence);
        assert_eq!(h("classDiagram-v2").kind, Kind::Class);
        assert_eq!(h("stateDiagram-v2").kind, Kind::State);
        assert_eq!(h("stateDiagram").kind, Kind::State);
        let elk = lex(b"flowchart-elk TD").unwrap();
        assert_eq!(elk.header.kind, Kind::Flowchart);
        assert_eq!(elk.warnings.len(), 1);
        assert!(fails("graph XY").contains("unknown flowchart direction `XY`"));
    }

    #[test]
    fn statements_after_the_header_on_the_same_line() {
        assert_eq!(texts("graph TD;A-->B;"), vec![(1, "A-->B".to_string())]);
        assert_eq!(texts("flowchart LR; A-->B; B-->C"), vec![(1, "A-->B".to_string()), (1, "B-->C".to_string())]);
    }

    #[test]
    fn unsupported_and_unknown_types() {
        assert_eq!(fails("\n\ngantt\ntitle x"), "line 3: Mermaid gantt diagrams are not supported yet (supported: flowchart, sequenceDiagram, classDiagram, stateDiagram)");
        assert!(fails("sankey-beta").contains("Mermaid sankey-beta diagrams are not supported yet"));
        assert!(fails("C4Context").contains("not supported yet"));
        assert!(fails("pie title Pets").contains("Mermaid pie diagrams"));
        assert_eq!(fails("hello\nA-->B"), "line 1: expected a diagram type such as `flowchart TD`, found `hello`");
        assert!(fails("").contains("empty"));
        assert!(fails("%% only a comment\n\n").contains("empty"));
        assert!(fails("@startuml\nA -> B\n@enduml").contains("plantuml()"));
    }

    #[test]
    fn splits_on_semicolons_outside_strings_and_entities() {
        let t = texts("flowchart TD\nA[\"a;b\"]-->B; B-->C\nA[x#59; y]-->B;C[\"`m;d`\"]");
        let s: Vec<&str> = t.iter().map(|(_, s)| s.as_str()).collect();
        assert_eq!(s, ["A[\"a;b\"]-->B", "B-->C", "A[x#59; y]-->B", "C[\"`m;d`\"]"]);
        assert_eq!(texts("sequenceDiagram\nA->>B: hi; B->>A: yo").len(), 2);
        assert_eq!(texts("sequenceDiagram\nA->>B: smile #quot;x#quot;;").len(), 1);
    }

    #[test]
    fn class_diagrams_only_lose_a_trailing_semicolon() {
        let t = texts("classDiagram\nclass A;\nA : +f(a; b)\nA : x #59;");
        let s: Vec<&str> = t.iter().map(|(_, s)| s.as_str()).collect();
        assert_eq!(s, ["class A", "A : +f(a; b)", "A : x #59;"]);
    }

    #[test]
    fn multi_line_strings_keep_the_first_line_number() {
        let t = texts("flowchart TD\nA[\"one\ntwo\"] --> B\nC --> D");
        assert_eq!(t, vec![(2, "A[\"one\ntwo\"] --> B".to_string()), (4, "C --> D".to_string())]);
        let t = texts("flowchart TD\nA[\"`**one**\n  two`\"]");
        assert_eq!(t, vec![(2, "A[\"`**one**\ntwo`\"]".to_string())]);
    }

    #[test]
    fn an_unclosed_quote_does_not_swallow_the_file() {
        let t = texts("sequenceDiagram\nA->>B: 5\" screen\nB->>A: ok");
        assert_eq!(t.len(), 2);
        let t = texts("flowchart TD\nA[\"open\nB-->C");
        assert_eq!(t, vec![(2, "A[\"open".to_string()), (3, "B-->C".to_string())]);
    }

    #[test]
    fn accessibility_statements_are_dropped() {
        let t = texts("flowchart TD\naccTitle: Title\naccDescr: one line\naccDescr {\n  many\n  lines\n}\nA-->B");
        assert_eq!(t, vec![(8, "A-->B".to_string())]);
    }

    #[test]
    fn lines_are_trimmed_and_blanks_dropped() {
        assert_eq!(texts("sequenceDiagram\n\n   A->>B: x  \n\t\n"), vec![(3, "A->>B: x".to_string())]);
    }
}
