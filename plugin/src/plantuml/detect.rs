//! Works out which kind of diagram a source describes, from the `@start...` tag and
//! from the statements in it. Adding a diagram type means adding a `Kind`, teaching
//! `classify` its signature statements and adding an arm in `mod.rs`.

use super::diagrams::gcommon::{parse_link, Ref};
use super::lexer::{first_word, Line};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Sequence,
    Activity,
    Class,
    State,
    UseCase,
    Component,
    Object,
    /// Activity diagrams in the pre-2013 syntax, `(*) --> "step"`.
    LegacyActivity,
    /// A diagram family we do not draw at all (gantt, mindmap, ...).
    Other(String),
}

impl Kind {
    pub fn name(&self) -> &str {
        match self {
            Kind::Sequence => "sequence",
            Kind::Activity => "activity",
            Kind::Class => "class",
            Kind::State => "state",
            Kind::UseCase => "use case",
            Kind::Component => "component",
            Kind::Object => "object",
            Kind::LegacyActivity => "legacy activity",
            Kind::Other(n) => n,
        }
    }
}

/// Signals one line can give. A diagram usually has many lines; the strongest kind
/// wins, see `detect`.
#[derive(Default)]
struct Signals {
    class: Option<usize>,
    actor: Option<usize>,
    /// Only an inheritance or composition arrow, which use case diagrams use as well.
    class_arrow: Option<usize>,
    object: Option<usize>,
    state: Option<usize>,
    usecase: Option<usize>,
    activity: Option<usize>,
    component: Option<usize>,
    interface: Option<usize>,
    sequence: Option<usize>,
    arrow: Option<usize>,
    legacy_activity: Option<usize>,
    other: Option<(usize, String)>,
}

fn set(slot: &mut Option<usize>, no: usize) {
    slot.get_or_insert(no);
}

fn classify(l: &Line, s: &mut Signals) {
    let t = l.text.as_str();
    let lower = t.to_ascii_lowercase();
    let w = first_word(&lower);
    let w = w.trim_end_matches(['{', ':']);
    // A link whose first name is a keyword (`Object <|-- ArrayList`) is not a declaration.
    let w = if parse_link(t).is_some_and(|p| matches!(&p.left, Ref::Name(n) if n.eq_ignore_ascii_case(w))) { "" } else { w };
    if w == "actor" || w == "actor/" {
        set(&mut s.actor, l.no);
    }
    match w {
        "class" | "abstract" | "annotation" | "enum" | "struct" | "protocol" | "exception" | "metaclass" => set(&mut s.class, l.no),
        "interface" => set(&mut s.interface, l.no),
        "object" | "map" => set(&mut s.object, l.no),
        "state" => set(&mut s.state, l.no),
        "usecase" => set(&mut s.usecase, l.no),
        "component" | "node" | "cloud" | "folder" | "frame" | "artifact" | "storage" | "agent" | "card" | "file" | "hexagon" | "person" | "rectangle" => {
            // `rectangle` and `package` also group activities; the activity signals
            // decide in that case.
            set(&mut s.component, l.no)
        }
        "start" | "stop" | "detach" | "kill" | "fork" | "split" | "repeat" | "switch" | "partition" | "swimlane" => {
            if lower == w || lower.starts_with(&format!("{w} ")) || lower.starts_with(&format!("{w}(")) || w == "partition" {
                set(&mut s.activity, l.no)
            }
        }
        "if" | "while" if lower[w.len()..].trim_start().starts_with('(') => set(&mut s.activity, l.no),
        "salt" | "robust" | "concise" | "clock" | "binary" | "analog" => {
            let name = if w == "salt" { "salt (wireframe)" } else { "timing" };
            s.other.get_or_insert((l.no, name.to_string()));
        }
        "participant" | "boundary" | "control" | "activate" | "deactivate" | "autonumber" | "return" | "create" | "destroy" | "alt" | "opt" | "loop" | "par"
        | "critical" | "ref" | "box" | "newpage" | "hnote" | "rnote" | "database" | "collections" | "queue" | "actor" | "entity" => {
            set(&mut s.sequence, l.no)
        }
        _ => {}
    }
    if t.starts_with(':') && (t.ends_with(';') || t.ends_with('|') || t.ends_with('<') || t.ends_with('>') || t.ends_with('/') || t.ends_with(']') || t.ends_with('}')) {
        set(&mut s.activity, l.no);
    } else if t.starts_with(':') && t[1..].contains(':') {
        set(&mut s.usecase, l.no);
    }
    if t.starts_with('|') && t[1..].contains('|') && !t.starts_with("||") {
        set(&mut s.activity, l.no);
    }
    if t.starts_with('(') && !t.starts_with("()") && t.contains(')') {
        // `(label)` is a use case, except as an activity connector which stands alone
        // on its line; use case wins only if the activity signals are absent.
        set(&mut s.usecase, l.no);
    }
    // The ends of a link say what kind of diagram it is: `(Use)` and `:Actor:` are use
    // cases, `[Comp]` and `() I` components, `[*]` a state.
    if let Some(p) = parse_link(t) {
        for r in [&p.left, &p.right] {
            match r {
                Ref::Paren(n) if n.starts_with('*') => set(&mut s.legacy_activity, l.no),
                Ref::Paren(_) | Ref::Colon(_) => set(&mut s.usecase, l.no),
                Ref::Bracket(_) | Ref::Interface(_) => set(&mut s.component, l.no),
                Ref::Star => set(&mut s.state, l.no),
                Ref::Name(_) => {}
            }
        }
    }
    if t.starts_with('[') && !t.starts_with("[*]") && !t.starts_with("[->") && !t.starts_with("[-") && !t.starts_with("[<") && t.contains(']') {
        set(&mut s.component, l.no);
    }
    if t.contains("[*]") {
        set(&mut s.state, l.no);
    }
    if ["||--", "--||", "}o--", "--o{", "|o--", "--o|", "}|--", "--|{", "|{--", "--{|"].iter().any(|a| t.contains(a)) {
        s.other.get_or_insert((l.no, "entity relationship".to_string()));
    } else if ["<|--", "--|>", "<|..", "..|>", "*--", "--*", "o--", "--o"].iter().any(|a| t.contains(a)) {
        set(&mut s.class_arrow, l.no);
    }
    if t.contains("->") || t.contains("<-") || t.contains("-\\") || t.contains("-/") {
        set(&mut s.arrow, l.no);
    }
}

/// The diagram kind and the line number that decided it (for "not supported" errors).
pub fn detect(tag: &str, tag_line: usize, lines: &[Line]) -> Result<(Kind, usize), String> {
    if tag != "uml" {
        let kind = match tag {
            "gantt" | "mindmap" | "wbs" | "salt" | "json" | "yaml" | "ditaa" | "dot" | "regex" | "ebnf" | "math" | "latex" | "chronology" | "chen" | "git" | "nwdiag" | "board" | "wire" | "files" | "creole" | "hcl" => {
                Kind::Other(tag.to_string())
            }
            _ => Kind::Other(tag.to_string()),
        };
        return Ok((kind, tag_line));
    }
    let mut s = Signals::default();
    for l in lines {
        classify(l, &mut s);
    }
    // Families we do not draw win over everything: drawing them as something else
    // would be wrong, an error is not.
    if let Some((n, name)) = s.other.take() {
        return Ok((Kind::Other(name), n));
    }
    if let Some(n) = s.legacy_activity {
        return Ok((Kind::LegacyActivity, n));
    }
    if let Some(n) = s.class {
        return Ok((Kind::Class, n));
    }
    if let Some(n) = s.object {
        return Ok((Kind::Object, n));
    }
    if let Some(n) = s.state {
        return Ok((Kind::State, n));
    }
    // Activity beats use case: `(A)` connectors and `:x;` lines are not use cases.
    if let Some(n) = s.activity {
        return Ok((Kind::Activity, n));
    }
    if let Some(n) = s.usecase {
        return Ok((Kind::UseCase, n));
    }
    if let Some(n) = s.component {
        return Ok((Kind::Component, n));
    }
    if let Some(n) = s.class_arrow {
        // `actor A` with a generalization arrow is a use case diagram.
        return Ok(if s.actor.is_some() { (Kind::UseCase, s.actor.unwrap_or(n)) } else { (Kind::Class, n) });
    }
    if let Some(n) = s.sequence {
        return Ok((Kind::Sequence, n));
    }
    if let Some(n) = s.interface {
        return Ok((Kind::Class, n));
    }
    if let Some(n) = s.arrow {
        return Ok((Kind::Sequence, n));
    }
    match lines.iter().find(|l| !l.text.is_empty()) {
        None => Err(format!("line {tag_line}: the diagram is empty")),
        Some(l) => Err(format!("line {}: cannot tell which kind of diagram this is, unexpected \"{}\"", l.no, l.text)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(src: &str) -> Result<(Kind, usize), String> {
        let s = crate::plantuml::lexer::lex(src.as_bytes()).unwrap();
        detect(&s.tag, s.tag_line, &s.lines)
    }

    #[test]
    fn detects_kinds() {
        assert_eq!(kind("Alice -> Bob: hi").unwrap().0, Kind::Sequence);
        assert_eq!(kind("actor A\nA -> B").unwrap().0, Kind::Sequence);
        assert_eq!(kind("start\n:x;\nstop").unwrap().0, Kind::Activity);
        assert_eq!(kind("class A\nA <|-- B").unwrap(), (Kind::Class, 1));
        assert_eq!(kind("[*] --> A").unwrap().0, Kind::State);
        assert_eq!(kind("actor U\nU --> (Use)").unwrap().0, Kind::UseCase);
        assert_eq!(kind("component X\n[Y] --> X").unwrap().0, Kind::Component);
        assert_eq!(kind("@startgantt\n[T] lasts 2 days\n@endgantt").unwrap().0, Kind::Other("gantt".into()));
        assert!(kind("").unwrap_err().contains("empty"));
    }

    #[test]
    fn ambiguous_inputs_pick_the_most_likely_kind() {
        // `actor` and `database` are also sequence participants.
        assert_eq!(kind("actor A\nA -> B : hi").unwrap().0, Kind::Sequence);
        assert_eq!(kind("actor A\nA --> (Use)").unwrap().0, Kind::UseCase);
        assert_eq!(kind("actor A\nusecase U\nA --> U").unwrap().0, Kind::UseCase);
        assert_eq!(kind(":A: --> (B)").unwrap().0, Kind::UseCase);
        assert_eq!(kind("actor A\nactor B\nB <|-- A").unwrap().0, Kind::UseCase);
        assert_eq!(kind("database DB\nA -> DB : query").unwrap().0, Kind::Sequence);
        assert_eq!(kind("node X\ndatabase DB\nX --> DB").unwrap().0, Kind::Component);
        assert_eq!(kind("[A] --> [B]").unwrap().0, Kind::Component);
        assert_eq!(kind("() I\n[C] - I").unwrap().0, Kind::Component);
        assert_eq!(kind("interface I\nI <|.. C").unwrap().0, Kind::Class);
        assert_eq!(kind("interface I\n[C] --> I").unwrap().0, Kind::Component);
        assert_eq!(kind("A -> B : call() - x").unwrap().0, Kind::Sequence);
        assert_eq!(kind("A -> B\n[-> A").unwrap().0, Kind::Sequence);
        assert_eq!(kind("Object <|-- ArrayList").unwrap().0, Kind::Class);
        assert_eq!(kind("object o1\no1 : x = 1").unwrap().0, Kind::Object);
        assert_eq!(kind("state A\nA --> B").unwrap().0, Kind::State);
        assert_eq!(kind("[*] --> A\nA --> [*]").unwrap().0, Kind::State);
        assert_eq!(kind("start\n:a;\n(A)\nstop").unwrap().0, Kind::Activity);
        assert_eq!(kind("rectangle R {\n  [a]\n}").unwrap().0, Kind::Component);
        assert_eq!(kind("A <|-- B").unwrap().0, Kind::Class);
        assert_eq!(kind("class A\nA --> B").unwrap().0, Kind::Class);
        assert_eq!(kind("A ||--o{ B").unwrap().0, Kind::Other("entity relationship".into()));
        assert_eq!(kind("clock clk with period 1").unwrap().0, Kind::Other("timing".into()));
        assert!(kind("hello world").unwrap_err().starts_with("line 1:"));
    }
}
