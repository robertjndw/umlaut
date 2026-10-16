//! State diagrams: states, composite states with concurrent regions, pseudo states
//! (`[*]`, choice, fork/join, history, entry/exit points), transitions and notes.
//!
//! `[*]` means "the start of this scope" on the left of an arrow and "the end of this
//! scope" on the right, so every composite state (and the diagram itself) gets its own
//! start and end node, created on first use. Regions split a composite state's members
//! into blocks laid out side by side or on top of each other; they are entities of their
//! own that own the members.

use super::gcommon::*;
use crate::plantuml::creole;
use crate::plantuml::lexer::{err, first_word, rest_after_word, unquote, Cursor, Line};
use crate::plantuml::model::{Ctx, LabelId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SKind {
    State,
    Start,
    End,
    Choice,
    Fork,
    Join,
    History,
    DeepHistory,
    EntryPoint,
    ExitPoint,
}

impl SKind {
    fn from_stereo(s: &str) -> Option<SKind> {
        Some(match s.to_ascii_lowercase().as_str() {
            "choice" => SKind::Choice,
            "fork" => SKind::Fork,
            "join" => SKind::Join,
            "start" => SKind::Start,
            "end" => SKind::End,
            "history" => SKind::History,
            "history*" | "deephistory" => SKind::DeepHistory,
            "entrypoint" | "inputpin" | "expansioninput" => SKind::EntryPoint,
            "exitpoint" | "outputpin" | "expansionoutput" => SKind::ExitPoint,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct State {
    pub kind: SKind,
    pub name: Option<LabelId>,
    pub desc: Vec<LabelId>,
    pub look: Look,
}

pub use super::gcommon::Note;

#[derive(Debug, Clone)]
pub enum Def {
    State(State),
    /// One block of a composite state with concurrent regions.
    Region,
    Note(Note),
}

pub type Ent = super::gcommon::Ent<Def>;

#[derive(Debug)]
pub struct Model {
    pub ents: Vec<Ent>,
    pub links: Vec<Link>,
    pub lr: bool,
    /// Per composite state: whether its regions are stacked (`--`) or side by side (`||`).
    pub stacked: Vec<(usize, bool)>,
}

enum Scope {
    State(usize, usize),
    Region(usize),
}

struct P<'a> {
    ctx: &'a mut Ctx,
    ents: Vec<Ent>,
    links: Vec<Link>,
    scopes: Vec<Scope>,
    stacked: Vec<(usize, bool)>,
    last: Option<usize>,
    lr: bool,
}

pub fn parse(lines: &[Line], ctx: &mut Ctx) -> Result<Model, String> {
    let mut p = P { ctx, ents: Vec::new(), links: Vec::new(), scopes: Vec::new(), stacked: Vec::new(), last: None, lr: false };
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
        return err(lines.first().map_or(1, |l| l.no), "the state diagram is empty");
    }
    Ok(Model { ents: p.ents, links: p.links, lr: p.lr, stacked: p.stacked })
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

    fn name_label(&mut self, text: &str) -> LabelId {
        let base = self.ctx.skin.text(&["state"], 14.0, "#000000");
        self.ctx.reg.add(creole::runs(text, &base), None)
    }

    fn new_state(&mut self, key: &str, display: Option<&str>, kind: SKind) -> usize {
        let name = display.filter(|_| kind == SKind::State || matches!(kind, SKind::EntryPoint | SKind::ExitPoint)).map(|d| self.name_label(d));
        self.ents.push(Ent { key: key.to_string(), parent: self.scope_parent(), def: Def::State(State { kind, name, desc: Vec::new(), look: Look::default() }) });
        let idx = self.ents.len() - 1;
        self.last = Some(idx);
        idx
    }

    fn find(&self, key: &str) -> Option<usize> {
        self.ents.iter().position(|e| e.key == key && !key.is_empty() && !matches!(e.def, Def::Region))
    }

    fn pseudo(&mut self, key: String, kind: SKind) -> usize {
        match self.find(&key) {
            Some(i) => i,
            None => self.new_state(&key, None, kind),
        }
    }

    fn resolve(&mut self, r: &Ref, role: Option<bool>, line: usize) -> Result<usize, String> {
        match r {
            Ref::Star => {
                let is_end = role == Some(false);
                let key = format!("[*]{}@{}", if is_end { "end" } else { "start" }, self.scope_key());
                Ok(self.pseudo(key, if is_end { SKind::End } else { SKind::Start }))
            }
            Ref::Bracket(h) if h == "H" || h == "H*" => {
                let key = format!("[{h}]@{}", self.scope_key());
                Ok(self.pseudo(key, if h == "H" { SKind::History } else { SKind::DeepHistory }))
            }
            Ref::Name(n) | Ref::Bracket(n) | Ref::Paren(n) | Ref::Colon(n) | Ref::Interface(Some(n)) => {
                if let Some(i) = self.find(n) {
                    return Ok(i);
                }
                Ok(self.new_state(n, Some(n), SKind::State))
            }
            Ref::Interface(None) => err(line, "this reference is not valid in a state diagram"),
        }
    }

    fn statement(&mut self, l: &Line, cur: &mut Cursor) -> Result<(), String> {
        let text = l.text.as_str();
        let lower = text.to_ascii_lowercase();
        let word = first_word(&lower);
        match lower.as_str() {
            "left to right direction" => {
                self.lr = true;
                return Ok(());
            }
            "top to bottom direction" => {
                self.lr = false;
                return Ok(());
            }
            "}" => return self.close(l),
            "--" | "||" | "-- --" => return self.region(l, lower == "--" || lower == "-- --"),
            _ => {}
        }
        if word == "hide" || word == "show" {
            if lower.contains("empty description") {
                return Ok(());
            }
            self.ctx.warn(l.no, &format!("`{}` is not supported and is ignored", l.text));
            return Ok(());
        }
        if word == "note" || word == "floating" {
            if let Some(n) = parse_note(l, cur)? {
                return self.note(l, n);
            }
        }
        if word == "state" {
            return self.decl(l, rest_after_word(text));
        }
        if let Some(parts) = parse_link(text) {
            return self.link(l, parts);
        }
        if let Some((r, rest)) = scan_ref(text) {
            if let Some(desc) = rest.trim_start().strip_prefix(':') {
                if !matches!(r, Ref::Star) {
                    let idx = self.resolve(&r, None, l.no)?;
                    self.last = Some(idx);
                    return self.describe(l, idx, desc.trim());
                }
            }
        }
        err(l.no, format!("syntax error, cannot understand \"{text}\""))
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

    /// `--` or `||` inside a composite state starts a new region.
    fn region(&mut self, l: &Line, stacked: bool) -> Result<(), String> {
        let comp = match self.scopes.last() {
            Some(Scope::State(i, _)) => {
                let comp = *i;
                // First separator: what came before becomes the first region.
                let first = self.ents.len();
                self.ents.push(Ent { key: String::new(), parent: Some(comp), def: Def::Region });
                for e in self.ents[..first].iter_mut() {
                    if e.parent == Some(comp) {
                        e.parent = Some(first);
                    }
                }
                self.stacked.push((comp, stacked));
                comp
            }
            Some(Scope::Region(r)) => {
                let comp = self.ents[*r].parent.unwrap_or(*r);
                self.scopes.pop();
                comp
            }
            None => return err(l.no, "`--` and `||` only separate regions inside a composite state"),
        };
        self.ents.push(Ent { key: String::new(), parent: Some(comp), def: Def::Region });
        crate::plantuml::lexer::check_nesting(self.scopes.len(), l.no)?;
        self.scopes.push(Scope::Region(self.ents.len() - 1));
        Ok(())
    }

    /// `state Name`, `state "Long name" as Id <<choice>> #color {`.
    fn decl(&mut self, l: &Line, rest: &str) -> Result<(), String> {
        let (rest, opens) = match rest.strip_suffix('{') {
            Some(r) => (r.trim(), true),
            None => (rest, false),
        };
        let (rest, stereos) = take_stereos(rest);
        let (rest, look) = take_look(&rest);
        let (head, desc) = match crate::plantuml::lexer::find_outside_quotes(rest, ':') {
            Some(i) => (rest[..i].trim(), Some(rest[i + 1..].trim())),
            None => (rest.trim(), None),
        };
        let (display, key) = if let Some(q) = head.strip_prefix('"') {
            let Some(end) = q.find('"') else { return err(l.no, "the state name is missing its closing quote") };
            let after = q[end + 1..].trim();
            match after.strip_prefix("as ") {
                Some(a) => (q[..end].to_string(), a.trim().to_string()),
                None if after.is_empty() => (q[..end].to_string(), q[..end].to_string()),
                None => return err(l.no, format!("cannot understand \"{after}\" after the state name")),
            }
        } else if let Some(i) = head.find(" as ") {
            (unquote(head[i + 4..].trim()).to_string(), head[..i].trim().to_string())
        } else {
            (head.to_string(), head.to_string())
        };
        if key.is_empty() || key.contains(char::is_whitespace) {
            return err(l.no, format!("cannot understand the state name \"{head}\""));
        }
        let kind = stereos.iter().find_map(|s| SKind::from_stereo(&s.text)).unwrap_or(SKind::State);
        let idx = match self.find(&key) {
            Some(i) => i,
            None => self.new_state(&key, Some(&display), kind),
        };
        self.last = Some(idx);
        // A mention before the declaration gave the state the id as its name.
        if matches!(&self.ents[idx].def, Def::State(s) if s.kind != kind || (kind == SKind::State && display != key)) {
            let name = if matches!(kind, SKind::State | SKind::EntryPoint | SKind::ExitPoint) { Some(self.name_label(&display)) } else { None };
            if let Def::State(s) = &mut self.ents[idx].def {
                s.kind = kind;
                s.name = name;
            }
        }
        if look != Look::default() {
            if let Def::State(s) = &mut self.ents[idx].def {
                s.look = look;
            }
        }
        if let Some(d) = desc.filter(|d| !d.is_empty()) {
            self.describe(l, idx, d)?;
        }
        if opens {
            crate::plantuml::lexer::check_nesting(self.scopes.len(), l.no)?;
            self.scopes.push(Scope::State(idx, l.no));
        }
        Ok(())
    }

    fn describe(&mut self, l: &Line, idx: usize, text: &str) -> Result<(), String> {
        let base = self.ctx.skin.text(&["stateattribute"], 12.0, "#000000");
        let Some(id) = self.ctx.reg.add_opt(creole::runs(text, &base), None) else { return Ok(()) };
        match &mut self.ents[idx].def {
            Def::State(s) => s.desc.push(id),
            _ => return err(l.no, "descriptions can only be added to states"),
        }
        Ok(())
    }

    fn link(&mut self, l: &Line, p: LinkParts) -> Result<(), String> {
        let from = self.resolve(&p.left, Some(true), l.no)?;
        let to = self.resolve(&p.right, Some(false), l.no)?;
        let base = self.ctx.skin.text(&["statearrow", "arrow"], 13.0, "#000000");
        let lab = p.label.as_deref().and_then(|t| self.ctx.reg.add_opt(creole::runs(t, &base), None));
        self.links.push(Link { from, to, arrow: p.arrow, label: lab, label_dir: p.label_dir, from_card: None, to_card: None, line: l.no });
        Ok(())
    }

    fn note(&mut self, l: &Line, n: NoteSyntax) -> Result<(), String> {
        let base = self.ctx.skin.text(&["note"], 13.0, "#000000");
        let Some(label) = self.ctx.reg.add_opt(creole::runs(&n.text, &base), None) else { return err(l.no, "the note is empty") };
        let (target, on_link) = note_target(&n.place, self.last, self.links.len(), l.no, |r| self.resolve(r, None, l.no))?;
        let parent = target.and_then(|t| self.ents[t].parent).or_else(|| self.scope_parent());
        self.ents.push(Ent { key: n.alias.clone().unwrap_or_default(), parent, def: Def::Note(Note { label, look: n.look, on_link }) });
        if let Some(t) = target {
            self.links.push(note_link(self.ents.len() - 1, t, n.side, l.no));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plantuml::layout::graph::Dir;

    fn model(src: &str) -> Result<(Model, Ctx), String> {
        crate::plantuml::parse_with(src, parse)
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
    }

    #[test]
    fn declarations_and_descriptions() {
        let (m, ctx) = model("state \"Long name\" as L #pink\nstate C <<choice>>\nstate F <<fork>>\nL : line one\nL : line two\nstate S : inline").unwrap();
        let l = m.ents.iter().find(|e| e.key == "L").unwrap();
        let Def::State(s) = &l.def else { panic!() };
        assert_eq!(s.desc.len(), 2);
        assert!(s.look.fill.is_some());
        assert_eq!(creole::plain(&ctx.reg.labels[s.name.unwrap().0].runs), "Long name");
        assert!(kinds(&m).contains(&SKind::Choice) && kinds(&m).contains(&SKind::Fork));
        let Def::State(s) = &m.ents.iter().find(|e| e.key == "S").unwrap().def else { panic!() };
        assert_eq!(s.desc.len(), 1);
    }

    #[test]
    fn regions_own_their_members() {
        let (m, _) = model("state P {\n  state A\n  --\n  state B\n  --\n  state C\n}").unwrap();
        let regions: Vec<usize> = (0..m.ents.len()).filter(|&i| matches!(m.ents[i].def, Def::Region)).collect();
        assert_eq!(regions.len(), 3);
        let a = m.ents.iter().position(|e| e.key == "A").unwrap();
        assert_eq!(m.ents[a].parent, Some(regions[0]));
        let c = m.ents.iter().position(|e| e.key == "C").unwrap();
        assert_eq!(m.ents[c].parent, Some(regions[2]));
        assert_eq!(m.stacked.len(), 1);
        assert!(m.stacked[0].1);
        let (m, _) = model("state P {\n  state A\n  ||\n  state B\n}\nstate Q").unwrap();
        assert!(!m.stacked[0].1);
        assert_eq!(m.ents.last().unwrap().parent, None);
    }

    #[test]
    fn history_and_notes() {
        let (m, _) = model("state X {\n  [H] --> A\n  [H*] --> B\n}\nnote right of X : hello\nX --> Y : go\nnote on link : n").unwrap();
        assert!(kinds(&m).contains(&SKind::History) && kinds(&m).contains(&SKind::DeepHistory));
        assert_eq!(m.ents.iter().filter(|e| matches!(e.def, Def::Note(_))).count(), 2);
    }

    #[test]
    fn transitions_keep_hints_and_labels() {
        let (m, ctx) = model("A -down-> B : next\nB -> C\nC -[#red]-> A").unwrap();
        assert_eq!(m.links[0].arrow.dir, Some(Dir::Down));
        assert_eq!(creole::plain(&ctx.reg.labels[m.links[0].label.unwrap().0].runs), "next");
        assert_eq!(m.links[1].arrow.len, 1);
        assert!(m.links[2].arrow.color.is_some());
    }

    #[test]
    fn errors() {
        assert!(model("A --> B\nnonsense here").unwrap_err().starts_with("line 2: syntax error"));
        assert!(model("state A {\nstate B").unwrap_err().starts_with("line 1:"));
        assert!(model("}").unwrap_err().starts_with("line 1:"));
        assert!(model("--").unwrap_err().starts_with("line 1:"));
    }
}
