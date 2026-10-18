//! Mermaid state diagrams, filled into PlantUML's state model and drawn by its layout.
//!
//! Mermaid's `A : text` is not an extra line under the name but the text shown in the
//! box: the first description replaces the id, and only a second one turns the box into
//! a title with a divider and the rest below. `state "Long name" as A` is the same as
//! `A : Long name`. The mapping below follows that, so one description becomes the
//! state's name and PlantUML's `desc` lines only appear from the second on.
//!
//! Parsing is two-phase (see `mermaid/mod.rs`): statements first build a draft tree of
//! strings, then labels are registered, since `classDef`, `class` and `style` may come
//! after the state they style.

use std::collections::HashMap;

use super::css::{split_classes, take_classes, Styles};
use super::lexer::{parse_dir, Header};
use super::{ignored, text, theme};
use crate::plantuml::diagrams::gcommon::{note_link, note_target, parse_note, Arrow, Deco, Link, Ref};
use crate::plantuml::diagrams::state::{Def, Ent, Note, SKind, State};
use crate::plantuml::layout::graph::RankDir;
use crate::plantuml::layout::Body;
use crate::plantuml::lexer::{check_nesting, err, first_word, rest_after_word, Cursor, Line};
use crate::plantuml::model::{Ctx, Measured};
use crate::plantuml::skin::Skin;

pub type Model = crate::plantuml::diagrams::state::Model;

enum Draft {
    State { kind: SKind, descs: Vec<String> },
    Region,
    Note { text: String, on_link: Option<usize> },
}

struct DEnt {
    key: String,
    parent: Option<usize>,
    draft: Draft,
}

enum Scope {
    State(usize, usize),
    Region(usize),
}

struct P<'a> {
    ctx: &'a mut Ctx,
    ents: Vec<DEnt>,
    links: Vec<Link>,
    link_labels: Vec<Option<String>>,
    scopes: Vec<Scope>,
    stacked: Vec<(usize, bool)>,
    last: Option<usize>,
    dir: Option<RankDir>,
    styles: Styles,
    /// States by key, so transitions find their ends without a scan.
    index: HashMap<String, usize>,
}

/// A state reference in a transition: `[*]` or an id.
enum Id {
    Star,
    Name(String),
}

pub fn parse(lines: &[Line], header: &Header, ctx: &mut Ctx) -> Result<Model, String> {
    let mut p = P {
        ctx,
        ents: Vec::new(),
        links: Vec::new(),
        link_labels: Vec::new(),
        scopes: Vec::new(),
        stacked: Vec::new(),
        last: None,
        dir: header.dir,
        styles: Styles::default(),
        index: HashMap::new(),
    };
    let mut cur = Cursor::new(lines);
    while let Some(l) = cur.next() {
        if !l.text.is_empty() {
            p.statement(l, &mut cur)?;
        }
    }
    if let Some(Scope::State(_, no)) = p.scopes.iter().rev().find(|s| matches!(s, Scope::State(..))) {
        return err(*no, "this composite state is never closed");
    }
    if p.ents.is_empty() {
        return err(header.line, "Mermaid state diagrams need at least one state");
    }
    Ok(p.register())
}

fn is_id_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Length of the id at the start of `s`. A `-` or `.` inside an id is fine as long as
/// another id character follows, so `A-->B` still ends the id at the arrow.
pub(super) fn id_len(s: &str) -> usize {
    let mut cs = s.char_indices().peekable();
    while let Some((i, c)) = cs.next() {
        let next = cs.peek().map(|n| n.1);
        let ok = is_id_char(c) || (matches!(c, '-' | '.') && i > 0 && next.is_some_and(is_id_char));
        if !ok {
            return i;
        }
    }
    s.len()
}

/// `[*]` or an id, plus any `:::class` suffixes, and what follows.
fn scan_id(s: &str) -> Option<(Id, Vec<String>, &str)> {
    let s = s.trim_start();
    let (id, rest) = if let Some(r) = s.strip_prefix("[*]") {
        (Id::Star, r)
    } else {
        let n = id_len(s);
        if n == 0 {
            return None;
        }
        (Id::Name(s[..n].to_string()), &s[n..])
    };
    let (classes, rest) = take_classes(rest);
    Some((id, classes, rest))
}

impl P<'_> {
    fn scope_parent(&self) -> Option<usize> {
        self.scopes.last().map(|s| match s {
            Scope::State(i, _) | Scope::Region(i) => *i,
        })
    }

    fn scope_key(&self) -> String {
        self.scope_parent().map_or(String::new(), |i| i.to_string())
    }

    /// The state called `key`, created with `kind` (and made the last one) if it is new.
    fn find_or_new(&mut self, key: &str, kind: SKind) -> usize {
        if let Some(&i) = self.index.get(key) {
            return i;
        }
        self.ents.push(DEnt { key: key.to_string(), parent: self.scope_parent(), draft: Draft::State { kind, descs: Vec::new() } });
        let idx = self.ents.len() - 1;
        if !key.is_empty() {
            self.index.insert(key.to_string(), idx);
        }
        self.last = Some(idx);
        idx
    }

    fn state(&mut self, key: &str) -> usize {
        self.find_or_new(key, SKind::State)
    }

    /// A transition end; `[*]` is the start state as a source and the end state as a target.
    fn resolve_id(&mut self, id: &Id, classes: &[String], is_end: bool) -> usize {
        match id {
            Id::Star => {
                let key = format!("[*]{}@{}", if is_end { "end" } else { "start" }, self.scope_key());
                self.find_or_new(&key, if is_end { SKind::End } else { SKind::Start })
            }
            Id::Name(n) => {
                let i = self.state(n);
                self.styles.assign(n, classes);
                i
            }
        }
    }

    fn statement(&mut self, l: &Line, cur: &mut Cursor) -> Result<(), String> {
        let text = l.text.as_str();
        match text {
            "}" => return self.close(l),
            "--" => return self.region(l),
            _ => {}
        }
        let word = first_word(text);
        let rest = rest_after_word(text);
        // A state may be called like a keyword: `note --> B` is a transition.
        let is_keyword = !rest.starts_with("-->") && !rest.starts_with(':');
        if is_keyword {
            match word {
                "direction" => return self.direction(l, rest),
                "state" => return self.decl(l, rest),
                "note" => {
                    if let Some(n) = parse_note(l, cur)? {
                        return self.note(l, n);
                    }
                }
                "classDef" => {
                    self.styles.define(rest);
                    return Ok(());
                }
                "class" => {
                    let Some(()) = self.styles.assign_statement(rest) else { return err(l.no, format!("expected `class <states> <name>`, found \"{text}\"")) };
                    return Ok(());
                }
                "style" => {
                    let Some(()) = self.styles.style_statement(rest) else { return err(l.no, format!("expected `style <state> <css>`, found \"{text}\"")) };
                    return Ok(());
                }
                "click" | "callback" | "link" | "href" | "scale" => {
                    ignored(self.ctx, l, word);
                    return Ok(());
                }
                _ => {}
            }
        }
        if let Some((left, lc, after)) = scan_id(text) {
            let after = after.trim_start();
            if let Some(arrow) = after.strip_prefix("-->") {
                if let Some((right, rc, tail)) = scan_id(arrow) {
                    let tail = tail.trim();
                    let label = match tail.strip_prefix(':') {
                        Some(t) => Some(t.trim().to_string()).filter(|t| !t.is_empty()),
                        None if tail.is_empty() => None,
                        None => return err(l.no, format!("cannot understand \"{tail}\" after the transition")),
                    };
                    self.transition(l, (left, lc), (right, rc), label);
                    return Ok(());
                }
                return err(l.no, format!("expected a state after `-->` in \"{text}\""));
            }
            if after.is_empty() || after.starts_with(':') {
                let Id::Name(name) = &left else { return err(l.no, "`[*]` can only be used in a transition") };
                let idx = self.state(name);
                self.styles.assign(name, &lc);
                self.last = Some(idx);
                if let Some(d) = after.strip_prefix(':') {
                    self.describe(idx, d.trim());
                }
                return Ok(());
            }
        }
        err(l.no, format!("syntax error, cannot understand \"{text}\""))
    }

    fn direction(&mut self, l: &Line, rest: &str) -> Result<(), String> {
        let Some(d) = parse_dir(rest.trim()) else { return err(l.no, format!("unknown direction `{rest}`, use TB, BT, LR or RL")) };
        if self.scopes.is_empty() {
            self.dir = Some(d);
        } else {
            self.ctx.warn(l.no, "a direction inside a composite state is ignored");
        }
        Ok(())
    }

    fn close(&mut self, l: &Line) -> Result<(), String> {
        if matches!(self.scopes.last(), Some(Scope::Region(_))) {
            self.scopes.pop();
        }
        match self.scopes.pop() {
            Some(_) => Ok(()),
            None => err(l.no, "`}` without a matching `{`"),
        }
    }

    /// `--` inside a composite state starts a new concurrent region. Mermaid stacks them
    /// with a dashed divider in between.
    fn region(&mut self, l: &Line) -> Result<(), String> {
        let comp = match self.scopes.last() {
            Some(Scope::State(i, _)) => {
                let comp = *i;
                // First separator: what came before becomes the first region.
                let first = self.ents.len();
                self.ents.push(DEnt { key: String::new(), parent: Some(comp), draft: Draft::Region });
                for e in self.ents[..first].iter_mut() {
                    if e.parent == Some(comp) {
                        e.parent = Some(first);
                    }
                }
                self.stacked.push((comp, true));
                comp
            }
            Some(Scope::Region(r)) => {
                let comp = self.ents[*r].parent.unwrap_or(*r);
                self.scopes.pop();
                comp
            }
            None => return err(l.no, "`--` only separates regions inside a composite state"),
        };
        self.ents.push(DEnt { key: String::new(), parent: Some(comp), draft: Draft::Region });
        check_nesting(self.scopes.len(), l.no)?;
        self.scopes.push(Scope::Region(self.ents.len() - 1));
        Ok(())
    }

    /// `state Id`, `state "Long name" as Id`, `state Id <<choice>>`, `state Id {`.
    fn decl(&mut self, l: &Line, rest: &str) -> Result<(), String> {
        let (rest, opens) = match rest.strip_suffix('{') {
            Some(r) => (r.trim(), true),
            None => (rest, false),
        };
        let mut kind = SKind::State;
        let mut head = rest.to_string();
        for (open, close) in [("<<", ">>"), ("[[", "]]")] {
            if let Some(a) = head.find(open) {
                let Some(b) = head[a..].find(close) else { return err(l.no, "the state type is missing its closing brackets") };
                let word = head[a + 2..a + b].trim().to_ascii_lowercase();
                kind = match word.as_str() {
                    "choice" => SKind::Choice,
                    "fork" => SKind::Fork,
                    "join" => SKind::Join,
                    other => return err(l.no, format!("unknown state type `{other}`, use choice, fork or join")),
                };
                head.replace_range(a..a + b + 2, "");
            }
        }
        let head = head.trim();
        let (display, key, inline_desc) = if let Some(q) = head.strip_prefix('"') {
            let Some(end) = q.find('"') else { return err(l.no, "the state name is missing its closing quote") };
            let after = q[end + 1..].trim();
            match after.strip_prefix("as ") {
                Some(a) => (Some(q[..end].to_string()), a.trim().to_string(), None),
                None => return err(l.no, format!("expected `as <id>` after the quoted name, found \"{after}\"")),
            }
        } else {
            match head.split_once(':') {
                Some((k, d)) => (None, k.trim().to_string(), Some(d.trim().to_string())),
                None => (None, head.to_string(), None),
            }
        };
        let (key, classes) = {
            let (k, c) = split_classes(&key);
            (k.to_string(), c)
        };
        if key.is_empty() || id_len(&key) != key.len() {
            return err(l.no, format!("cannot understand the state id \"{key}\", ids may hold letters, digits and `_`"));
        }
        let idx = self.state(&key);
        self.styles.assign(&key, &classes);
        self.last = Some(idx);
        if let Draft::State { kind: k, .. } = &mut self.ents[idx].draft {
            if kind != SKind::State {
                *k = kind;
            }
        }
        if let Some(d) = display {
            self.describe(idx, &d);
        }
        if let Some(d) = inline_desc.filter(|d| !d.is_empty()) {
            self.describe(idx, &d);
        }
        if opens {
            check_nesting(self.scopes.len(), l.no)?;
            self.scopes.push(Scope::State(idx, l.no));
        }
        Ok(())
    }

    fn describe(&mut self, idx: usize, text: &str) {
        if let Draft::State { descs, .. } = &mut self.ents[idx].draft {
            if !text.is_empty() {
                descs.push(text.to_string());
            }
        }
    }

    fn transition(&mut self, l: &Line, left: (Id, Vec<String>), right: (Id, Vec<String>), label: Option<String>) {
        let from = self.resolve_id(&left.0, &left.1, false);
        let to = self.resolve_id(&right.0, &right.1, true);
        let mut arrow = Arrow::plain(2);
        arrow.head = Deco::Arrow;
        self.links.push(Link { from, to, arrow, label: None, label_dir: None, from_card: None, to_card: None, line: l.no });
        self.link_labels.push(label);
    }

    fn note(&mut self, l: &Line, n: crate::plantuml::diagrams::gcommon::NoteSyntax) -> Result<(), String> {
        if n.text.trim().is_empty() {
            return err(l.no, "the note is empty");
        }
        let (target, on_link) = note_target(&n.place, self.last, self.links.len(), l.no, |r| match r {
            Ref::Name(name) => Ok(self.state(name)),
            _ => err(l.no, "a note can only be attached to a state"),
        })?;
        let parent = target.and_then(|t| self.ents[t].parent).or_else(|| self.scope_parent());
        self.ents.push(DEnt { key: n.alias.clone().unwrap_or_default(), parent, draft: Draft::Note { text: n.text, on_link } });
        if let Some(t) = target {
            self.links.push(note_link(self.ents.len() - 1, t, n.side, l.no));
            self.link_labels.push(None);
        }
        Ok(())
    }

    /// Phase two: styles are known, so labels can be registered in their fixed order.
    fn register(self) -> Model {
        let P { ctx, ents, mut links, link_labels, stacked, dir, styles, .. } = self;
        let (note_base, state_base) = (theme::note(&ctx.skin), theme::state(&ctx.skin));
        let mut out = Vec::with_capacity(ents.len());
        for e in ents {
            let def = match e.draft {
                Draft::Region => Def::Region,
                Draft::Note { text, on_link } => {
                    let label = ctx.reg.add(text::runs(&text, &note_base), None);
                    Def::Note(Note { label, look: Default::default(), on_link })
                }
                Draft::State { kind, descs } => {
                    let css = styles.resolve(&e.key);
                    let look = css.look();
                    let base = css.text(&state_base);
                    let mut descs = descs.into_iter();
                    let shows_name = kind == SKind::State;
                    let name_text = descs.next().unwrap_or_else(|| e.key.clone());
                    let name = shows_name.then(|| ctx.reg.add(text::runs(&name_text, &base), None));
                    let desc = if shows_name { descs.filter_map(|d| ctx.reg.add_opt(text::runs(&d, &base), None)).collect() } else { Vec::new() };
                    Def::State(State { kind, name, desc, look })
                }
            };
            out.push(Ent { key: e.key, parent: e.parent, def });
        }
        let edge = theme::edge(&ctx.skin);
        for (link, label) in links.iter_mut().zip(link_labels) {
            link.label = label.and_then(|t| ctx.reg.add_opt(text::runs(&t, &edge), None));
        }
        Model { ents: out, links, dir: dir.unwrap_or_default(), stacked }
    }
}

pub fn layout(m: &Model, skin: &Skin, meas: &Measured) -> Result<Body, String> {
    crate::plantuml::layout::state::layout(m, skin, meas)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plantuml::creole;

    fn model(src: &str) -> Result<(Model, Ctx), String> {
        let mut f = super::super::front(format!("stateDiagram-v2\n{src}").as_bytes())?;
        let m = parse(&f.lines, &f.header, &mut f.ctx)?;
        Ok((m, f.ctx))
    }

    fn state<'a>(m: &'a Model, key: &str) -> &'a State {
        match &m.ents.iter().find(|e| e.key == key).unwrap().def {
            Def::State(s) => s,
            _ => panic!("{key} is not a state"),
        }
    }

    fn plain(ctx: &Ctx, id: crate::plantuml::model::LabelId) -> String {
        creole::plain(&ctx.reg.labels[id.0].runs)
    }

    fn kinds(m: &Model) -> Vec<SKind> {
        m.ents.iter().filter_map(|e| if let Def::State(s) = &e.def { Some(s.kind) } else { None }).collect()
    }

    #[test]
    fn start_and_end_are_per_scope() {
        let (m, _) = model("[*] --> A\nstate A {\n  [*] --> B\n  B --> [*]\n}\nA --> [*]").unwrap();
        assert_eq!(kinds(&m).iter().filter(|k| **k == SKind::Start).count(), 2);
        assert_eq!(kinds(&m).iter().filter(|k| **k == SKind::End).count(), 2);
        let a = m.ents.iter().position(|e| e.key == "A").unwrap();
        let b = m.ents.iter().position(|e| e.key == "B").unwrap();
        assert_eq!(m.ents[b].parent, Some(a));
        assert_eq!(m.links[0].arrow.head, Deco::Arrow);
        assert_eq!(m.links[0].arrow.len, 2);
    }

    #[test]
    fn first_description_replaces_the_id() {
        let (m, ctx) = model("A\nB : only one\nstate \"Long name\" as C\nD : first\nD : second\nD : third").unwrap();
        assert_eq!(plain(&ctx, state(&m, "A").name.unwrap()), "A");
        let b = state(&m, "B");
        assert_eq!(plain(&ctx, b.name.unwrap()), "only one");
        assert!(b.desc.is_empty());
        assert_eq!(plain(&ctx, state(&m, "C").name.unwrap()), "Long name");
        let d = state(&m, "D");
        assert_eq!(plain(&ctx, d.name.unwrap()), "first");
        assert_eq!(d.desc.len(), 2);
    }

    #[test]
    fn pseudo_states_have_no_name() {
        let (m, _) = model("state c <<choice>>\nstate f <<fork>>\nstate j <<join>>\nstate k [[fork]]\nA --> c").unwrap();
        assert_eq!(state(&m, "c").kind, SKind::Choice);
        assert_eq!(state(&m, "f").kind, SKind::Fork);
        assert_eq!(state(&m, "j").kind, SKind::Join);
        assert_eq!(state(&m, "k").kind, SKind::Fork);
        assert!(state(&m, "c").name.is_none());
        assert!(model("state c <<nope>>").unwrap_err().starts_with("line 2: unknown state type"));
    }

    #[test]
    fn concurrent_regions_are_stacked() {
        let (m, _) = model("state P {\n  A --> B\n  --\n  C --> D\n  --\n  E\n}").unwrap();
        let regions: Vec<usize> = (0..m.ents.len()).filter(|&i| matches!(m.ents[i].def, Def::Region)).collect();
        assert_eq!(regions.len(), 3);
        let a = m.ents.iter().position(|e| e.key == "A").unwrap();
        assert_eq!(m.ents[a].parent, Some(regions[0]));
        let e = m.ents.iter().position(|e| e.key == "E").unwrap();
        assert_eq!(m.ents[e].parent, Some(regions[2]));
        assert_eq!(m.stacked, vec![(0, true)]);
        assert!(model("A --> B\n--").unwrap_err().starts_with("line 3:"));
    }

    #[test]
    fn notes_single_and_block() {
        let (m, ctx) = model("A --> B : go\nnote right of A : hello\nnote left of B\n  first\n  second\nend note").unwrap();
        let notes: Vec<_> = m.ents.iter().filter_map(|e| if let Def::Note(n) = &e.def { Some(n) } else { None }).collect();
        assert_eq!(notes.len(), 2);
        assert_eq!(plain(&ctx, notes[0].label), "hello");
        assert_eq!(plain(&ctx, notes[1].label), "first\nsecond");
        assert_eq!(m.links.len(), 3);
        assert!(model("A\nnote left of A\nnever closed").unwrap_err().contains("never closed"));
    }

    #[test]
    fn styles_come_before_or_after() {
        let (m, ctx) = model("A:::hot --> B\nclassDef hot fill:#f00,color:#fff,stroke-width:3px\nclass B hot\nstyle A stroke:#00f").unwrap();
        for k in ["A", "B"] {
            let s = state(&m, k);
            assert_eq!(s.look.fill.as_ref().unwrap().color, "#ff0000ff");
            assert!(s.look.bold);
            assert_eq!(ctx.reg.labels[s.name.unwrap().0].runs[0].color, "#ffffffff");
        }
        assert_eq!(state(&m, "A").look.line.as_deref(), Some("#0000ffff"));
        assert!(state(&m, "B").look.line.is_none());
    }

    #[test]
    fn directions() {
        let (m, _) = model("direction RL\nA --> B").unwrap();
        assert_eq!(m.dir, RankDir::RL);
        let (m, _) = model("A --> B").unwrap();
        assert_eq!(m.dir, RankDir::TB);
        let (m, ctx) = model("state X {\n  direction LR\n  A\n}\ndirection BT").unwrap();
        assert_eq!(m.dir, RankDir::BT);
        assert_eq!(ctx.warnings.len(), 1);
        assert!(model("direction XY").is_err());
    }

    #[test]
    fn transitions_and_labels() {
        let (m, ctx) = model("A-->B:go now\nB --> C\nA : x: y").unwrap();
        assert_eq!(plain(&ctx, m.links[0].label.unwrap()), "go now");
        assert!(m.links[1].label.is_none());
        assert_eq!(plain(&ctx, state(&m, "A").name.unwrap()), "x: y");
        let (m, _) = model("state-1 --> state_2").unwrap();
        assert_eq!(m.ents[0].key, "state-1");
    }

    #[test]
    fn registration_order_is_stable() {
        let (_, ctx) = model("A --> B : t\nA : d\nclassDef c color:red\nclass B c").unwrap();
        let texts: Vec<String> = ctx.reg.labels.iter().map(|l| creole::plain(&l.runs)).collect();
        assert_eq!(texts, ["d", "B", "t"]);
    }

    #[test]
    fn ignored_and_errors() {
        let (_, ctx) = model("A --> B\nclick A call cb()").unwrap();
        assert_eq!(ctx.warnings, vec!["line 3: `click` is not supported and is ignored".to_string()]);
        assert!(model("A --> B\nnonsense here").unwrap_err().starts_with("line 3: syntax error"));
        assert!(model("hide empty description\nA").unwrap_err().starts_with("line 2:"));
        assert!(model("state A {\nstate B").unwrap_err().starts_with("line 2:"));
        assert!(model("}").unwrap_err().starts_with("line 2:"));
        assert!(model("A --> ").unwrap_err().starts_with("line 2:"));
        assert!(model("%% nothing").is_err());
        assert!(model("").unwrap_err().contains("Mermaid state diagrams"));
    }
}
