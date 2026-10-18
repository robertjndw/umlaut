//! Use case, component and deployment diagrams. PlantUML parses them with one grammar
//! (actors, use cases, components, interfaces and the deployment shapes can all appear in
//! one picture), so they share this parser and `layout::description`.
//!
//! Anything with a body, `node X { ... }`, `rectangle System { ... }`, becomes a
//! container; without a body the same keyword is a plain shape. Names that are only
//! mentioned in a link get a kind from their company: an actor in a use case diagram,
//! an interface next to a `[component]`, a rectangle otherwise.

use super::gcommon::*;
use crate::plantuml::creole;
use crate::plantuml::layout::graph::RankDir;
use crate::plantuml::lexer::{err, find_outside_quotes, first_word, rest_after_word, Cursor, Line};
use crate::plantuml::model::{Ctx, LabelId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DKind {
    Actor,
    UseCase,
    Component,
    Interface,
    Node,
    Database,
    Cloud,
    Package,
    Folder,
    Frame,
    Artifact,
    Storage,
    Card,
    File,
    Queue,
    Stack,
    Rectangle,
    Agent,
    Boundary,
    Control,
    Entity,
    Port,
    PortIn,
    PortOut,
    Person,
    Hexagon,
    Label,
    Collections,
}

impl DKind {
    fn from_word(w: &str) -> Option<DKind> {
        Some(match w {
            "actor" => DKind::Actor,
            "usecase" => DKind::UseCase,
            "component" => DKind::Component,
            "interface" => DKind::Interface,
            "node" => DKind::Node,
            "database" => DKind::Database,
            "cloud" => DKind::Cloud,
            "package" | "namespace" => DKind::Package,
            "folder" => DKind::Folder,
            "frame" => DKind::Frame,
            "artifact" => DKind::Artifact,
            "storage" => DKind::Storage,
            "card" => DKind::Card,
            "file" => DKind::File,
            "queue" => DKind::Queue,
            "stack" => DKind::Stack,
            "rectangle" => DKind::Rectangle,
            "agent" => DKind::Agent,
            "boundary" => DKind::Boundary,
            "control" => DKind::Control,
            "entity" => DKind::Entity,
            "port" => DKind::Port,
            "portin" => DKind::PortIn,
            "portout" => DKind::PortOut,
            "person" => DKind::Person,
            "hexagon" => DKind::Hexagon,
            "label" => DKind::Label,
            "collections" => DKind::Collections,
            _ => return None,
        })
    }

    /// Skinparam prefix of the kind.
    pub fn prefix(self) -> &'static str {
        match self {
            DKind::Actor => "actor",
            DKind::UseCase => "usecase",
            DKind::Component => "component",
            DKind::Interface => "interface",
            DKind::Node => "node",
            DKind::Database => "database",
            DKind::Cloud => "cloud",
            DKind::Package => "package",
            DKind::Folder => "folder",
            DKind::Frame => "frame",
            DKind::Artifact => "artifact",
            DKind::Storage => "storage",
            DKind::Card => "card",
            DKind::File => "file",
            DKind::Queue => "queue",
            DKind::Stack => "stack",
            DKind::Rectangle => "rectangle",
            DKind::Agent => "agent",
            DKind::Boundary => "boundary",
            DKind::Control => "control",
            DKind::Entity => "entity",
            DKind::Port | DKind::PortIn | DKind::PortOut => "port",
            DKind::Person => "person",
            DKind::Hexagon => "hexagon",
            DKind::Label => "label",
            DKind::Collections => "collections",
        }
    }

    /// The outline a container of this kind gets.
    pub fn group_style(self) -> GroupStyle {
        match self {
            DKind::Package => GroupStyle::Package,
            DKind::Folder => GroupStyle::Folder,
            DKind::Frame => GroupStyle::Frame,
            DKind::Node => GroupStyle::Node,
            DKind::Cloud => GroupStyle::Cloud,
            DKind::Database => GroupStyle::Database,
            _ => GroupStyle::Rectangle,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Elem {
    pub kind: DKind,
    pub name: LabelId,
    pub stereo: Option<LabelId>,
    pub look: Look,
    /// `actor/` and `usecase/`.
    pub business: bool,
}

pub use super::gcommon::Note;

#[derive(Debug, Clone)]
pub enum Def {
    Elem(Elem),
    Note(Note),
}

pub type Ent = super::gcommon::Ent<Def>;

#[derive(Debug)]
pub struct Model {
    pub ents: Vec<Ent>,
    pub links: Vec<Link>,
    pub dir: RankDir,
}

struct P<'a> {
    ctx: &'a mut Ctx,
    ents: Vec<Ent>,
    links: Vec<Link>,
    scopes: Vec<(usize, usize)>,
    last: Option<usize>,
    dir: RankDir,
    /// Whether the diagram has use case syntax, which decides what a bare name is.
    usecase_world: bool,
}

pub fn parse(lines: &[Line], ctx: &mut Ctx) -> Result<Model, String> {
    let usecase_world = lines.iter().any(|l| {
        let t = l.text.as_str();
        let w = first_word(t).to_ascii_lowercase();
        w == "usecase" || w == "usecase/" || (t.starts_with('(') && !t.starts_with("()")) || t.starts_with(':') || t.contains("> (") || t.contains("- (")
    });
    let mut p = P { ctx, ents: Vec::new(), links: Vec::new(), scopes: Vec::new(), last: None, dir: RankDir::TB, usecase_world };
    let mut cur = Cursor::new(lines);
    while let Some(l) = cur.next() {
        if !l.text.is_empty() {
            p.statement(l, &mut cur)?;
        }
    }
    if let Some((_, no)) = p.scopes.last() {
        return err(*no, "this block is never closed");
    }
    if p.ents.is_empty() {
        return err(lines.first().map_or(1, |l| l.no), "the diagram is empty");
    }
    Ok(Model { ents: p.ents, links: p.links, dir: p.dir })
}

/// `<<include>>` in text becomes the guillemets PlantUML draws.
fn guillemets(s: &str) -> String {
    s.replace("<<", "\u{ab}").replace(">>", "\u{bb}")
}

/// The name inside `(Use)`, `:Actor:`, `[Comp]` or quotes; plain names come back as they are.
fn bare(s: &str) -> String {
    let t = s.trim();
    for (a, b) in [("(", ")"), (":", ":"), ("[", "]"), ("\"", "\"")] {
        if let Some(i) = t.strip_prefix(a).and_then(|x| x.strip_suffix(b)) {
            return i.trim().to_string();
        }
    }
    t.to_string()
}

impl P<'_> {
    fn scope_parent(&self) -> Option<usize> {
        self.scopes.last().map(|s| s.0)
    }

    fn name_label(&mut self, kind: DKind, text: &str) -> LabelId {
        let base = self.ctx.skin.text(&[kind.prefix()], 14.0, "#000000");
        self.ctx.reg.add(creole::runs(text, &base), None)
    }

    fn new_elem(&mut self, key: &str, display: &str, kind: DKind) -> usize {
        let name = self.name_label(kind, display);
        self.ents.push(Ent { key: key.to_string(), parent: self.scope_parent(), def: Def::Elem(Elem { kind, name, stereo: None, look: Look::default(), business: false }) });
        let idx = self.ents.len() - 1;
        self.last = Some(idx);
        idx
    }

    fn find(&self, key: &str) -> Option<usize> {
        self.ents.iter().position(|e| e.key == key && !key.is_empty())
    }

    fn resolve(&mut self, r: &Ref, other: &Ref, line: usize) -> Result<usize, String> {
        let (name, kind) = match r {
            Ref::Bracket(n) => (n.as_str(), DKind::Component),
            Ref::Paren(n) => (n.as_str(), DKind::UseCase),
            Ref::Colon(n) => (n.as_str(), DKind::Actor),
            Ref::Interface(Some(n)) => (n.as_str(), DKind::Interface),
            Ref::Name(n) => {
                let kind = if self.usecase_world {
                    DKind::Actor
                } else if matches!(other, Ref::Bracket(_) | Ref::Interface(_)) {
                    DKind::Interface
                } else {
                    DKind::Rectangle
                };
                (n.as_str(), kind)
            }
            Ref::Interface(None) | Ref::Star => return err(line, "this reference is not valid here"),
        };
        match self.find(name) {
            Some(i) => Ok(i),
            None => Ok(self.new_elem(name, name, kind)),
        }
    }

    fn statement(&mut self, l: &Line, cur: &mut Cursor) -> Result<(), String> {
        let text = l.text.as_str();
        let lower = text.to_ascii_lowercase();
        let word = first_word(&lower).trim_end_matches('{');
        match lower.as_str() {
            "left to right direction" => {
                self.dir = RankDir::LR;
                return Ok(());
            }
            "top to bottom direction" => {
                self.dir = RankDir::TB;
                return Ok(());
            }
            "}" => {
                return match self.scopes.pop() {
                    Some(_) => Ok(()),
                    None => err(l.no, "`}` without a matching `{`"),
                };
            }
            _ => {}
        }
        if word == "hide" || word == "show" || word == "set" || word == "allowmixing" || word == "allow_mixing" {
            self.ctx.warn(l.no, &format!("`{}` is not supported and is ignored", l.text));
            return Ok(());
        }
        if word == "note" || word == "floating" {
            if let Some(n) = parse_note(l, cur)? {
                return self.note(l, n);
            }
        }
        let kw = word.trim_end_matches('/');
        if let Some(kind) = DKind::from_word(kw) {
            let rest = rest_after_word(text);
            // `actor` followed by an arrow is a name, not a declaration.
            if !rest.is_empty() && !(parse_link(text).is_some() && !rest.starts_with('"')) {
                return self.decl(l, kind, rest, word.ends_with('/'));
            }
        }
        if let Some(parts) = parse_link(text) {
            return self.link(l, parts);
        }
        if let Some((r, rest)) = scan_ref(text) {
            let kind = match r {
                Ref::Bracket(_) => Some(DKind::Component),
                Ref::Paren(_) => Some(DKind::UseCase),
                Ref::Colon(_) => Some(DKind::Actor),
                Ref::Interface(_) => Some(DKind::Interface),
                _ => None,
            };
            if let Some(kind) = kind {
                let (rest, _) = take_stereos(rest);
                if rest.trim().is_empty() || rest.trim().starts_with("as ") || rest.trim().starts_with('#') || rest.trim().starts_with('{') {
                    let text = if kind == DKind::Interface { text.trim_start_matches("()").trim() } else { text };
                    return self.decl(l, kind, text, false);
                }
            }
        }
        err(l.no, format!("syntax error, cannot understand \"{text}\""))
    }

    fn decl(&mut self, l: &Line, kind: DKind, rest: &str, business: bool) -> Result<(), String> {
        let (rest, opens) = match rest.trim().strip_suffix('{') {
            Some(r) => (r.trim(), true),
            None => (rest.trim(), false),
        };
        let (rest, stereos) = take_stereos(rest);
        let (rest, look) = take_look(&rest);
        let (display, key) = match rest.rfind(" as ") {
            Some(i) if find_outside_quotes(&rest[i..], '"').is_none() || rest[i..].matches('"').count() % 2 == 0 => {
                let (a, b) = (rest[..i].trim(), rest[i + 4..].trim());
                // `Name as "Display"` has the display on the right.
                if b.starts_with('"') { (bare(b), bare(a)) } else { (bare(a), bare(b)) }
            }
            _ => (bare(rest), bare(rest)),
        };
        // `cloud {` has no name; it still needs a key of its own.
        let key = if key.is_empty() && opens { format!("{}@{}", kind.prefix(), l.no) } else { key };
        if key.is_empty() {
            return err(l.no, "this declaration has no name");
        }
        let idx = match self.find(&key) {
            Some(i) => {
                let name = self.name_label(kind, &display);
                if let Def::Elem(e) = &mut self.ents[i].def {
                    e.kind = kind;
                    e.name = name;
                }
                i
            }
            None => self.new_elem(&key, &display, kind),
        };
        self.last = Some(idx);
        let stereo_text: Vec<String> = stereos.iter().filter(|s| !s.text.is_empty()).map(|s| format!("\u{ab}{}\u{bb}", s.text)).collect();
        let stereo = if stereo_text.is_empty() {
            None
        } else {
            let base = self.ctx.skin.text(&["stereotype"], 12.0, "#000000").italic();
            Some(self.ctx.reg.add(creole::runs(&stereo_text.join("\n"), &base), None))
        };
        // `<<Folder>>` and the like pick a container style for a package.
        let kind = if kind == DKind::Package { stereos.iter().find_map(|s| DKind::from_word(&s.text.to_ascii_lowercase())).filter(|k| matches!(k, DKind::Folder | DKind::Frame | DKind::Node | DKind::Cloud | DKind::Database | DKind::Rectangle)).unwrap_or(kind) } else { kind };
        if let Def::Elem(e) = &mut self.ents[idx].def {
            e.kind = kind;
            e.business |= business;
            if stereo.is_some() && !(kind != DKind::Package && stereos.iter().all(|s| DKind::from_word(&s.text.to_ascii_lowercase()).is_some())) {
                e.stereo = stereo;
            }
            if look != Look::default() {
                e.look = look;
            }
        }
        if opens {
            crate::plantuml::lexer::check_nesting(self.scopes.len(), l.no)?;
            self.scopes.push((idx, l.no));
        }
        Ok(())
    }

    fn link(&mut self, l: &Line, p: LinkParts) -> Result<(), String> {
        let from = self.resolve(&p.left, &p.right, l.no)?;
        let to = self.resolve(&p.right, &p.left, l.no)?;
        let base = self.ctx.skin.text(&["arrow"], 13.0, "#000000");
        let lab = p.label.as_deref().and_then(|t| self.ctx.reg.add_opt(creole::runs(&guillemets(t), &base), None));
        self.links.push(Link { from, to, arrow: p.arrow, label: lab, label_dir: p.label_dir, from_card: None, to_card: None, line: l.no });
        Ok(())
    }

    fn note(&mut self, l: &Line, n: NoteSyntax) -> Result<(), String> {
        let base = self.ctx.skin.text(&["note"], 13.0, "#000000");
        let Some(label) = self.ctx.reg.add_opt(creole::runs(&n.text, &base), None) else { return err(l.no, "the note is empty") };
        let (target, on_link) = note_target(&n.place, self.last, self.links.len(), l.no, |r| self.resolve(r, r, l.no))?;
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

    fn model(src: &str) -> Result<(Model, Ctx), String> {
        crate::plantuml::parse_with(src, parse)
    }

    fn kind(m: &Model, key: &str) -> DKind {
        match &m.ents.iter().find(|e| e.key == key).unwrap().def {
            Def::Elem(e) => e.kind,
            _ => panic!(),
        }
    }

    fn text(ctx: &Ctx, id: LabelId) -> String {
        creole::plain(&ctx.reg.labels[id.0].runs)
    }

    #[test]
    fn use_case_forms() {
        let (m, ctx) = model(":Main Admin: as Admin\n(Use the application) as (Use)\nactor Guest\nactor \"Long name\" as LN\nusecase U2\nusecase \"Other\" as U3\nUser -> (Start)\nUser --> (Use)\nAdmin ---> (Use)").unwrap();
        assert_eq!(kind(&m, "Admin"), DKind::Actor);
        assert_eq!(kind(&m, "Use"), DKind::UseCase);
        assert_eq!(kind(&m, "User"), DKind::Actor);
        assert_eq!(kind(&m, "Start"), DKind::UseCase);
        assert_eq!(kind(&m, "U3"), DKind::UseCase);
        let Def::Elem(e) = &m.ents.iter().find(|e| e.key == "Admin").unwrap().def else { panic!() };
        assert_eq!(text(&ctx, e.name), "Main Admin");
        let Def::Elem(e) = &m.ents.iter().find(|e| e.key == "Use").unwrap().def else { panic!() };
        assert_eq!(text(&ctx, e.name), "Use the application");
        assert_eq!(m.links.len(), 3);
        assert_eq!(m.links[2].arrow.len, 3);
    }

    #[test]
    fn include_labels_use_guillemets() {
        let (m, ctx) = model("(Login) .> (Check) : <<include>>").unwrap();
        assert_eq!(text(&ctx, m.links[0].label.unwrap()), "\u{ab}include\u{bb}");
    }

    #[test]
    fn component_forms() {
        let (m, _) = model("DataAccess - [First Component]\n[First Component] ..> HTTP : use\ncomponent C1\ncomponent [C2] as X\ninterface I1\n() \"I 2\" as I2\nnode N <<Folder>>\ndatabase DB").unwrap();
        assert_eq!(kind(&m, "DataAccess"), DKind::Interface);
        assert_eq!(kind(&m, "First Component"), DKind::Component);
        assert_eq!(kind(&m, "X"), DKind::Component);
        assert_eq!(kind(&m, "I2"), DKind::Interface);
        assert_eq!(kind(&m, "DB"), DKind::Database);
        assert_eq!(m.links[0].arrow.len, 1);
    }

    #[test]
    fn containers_nest() {
        let m = model("package \"Some Group\" {\n  HTTP - [First]\n  [Another]\n}\nnode \"Other\" {\n  database \"My Sql\" {\n    folder F {\n      [F3]\n    }\n  }\n}\ncloud {\n}").unwrap().0;
        let idx = |k: &str| m.ents.iter().position(|e| e.key == k).unwrap();
        assert_eq!(m.ents[idx("First")].parent, Some(idx("Some Group")));
        assert_eq!(m.ents[idx("F3")].parent, Some(idx("F")));
        assert_eq!(m.ents[idx("F")].parent, Some(idx("My Sql")));
    }

    #[test]
    fn stereotypes_and_colors() {
        let (m, ctx) = model("component C <<Service>> #pink\nrectangle \"System\" #lightblue {\n  usecase U\n}").unwrap();
        let Def::Elem(e) = &m.ents[0].def else { panic!() };
        assert_eq!(text(&ctx, e.stereo.unwrap()), "\u{ab}Service\u{bb}");
        assert!(e.look.fill.is_some());
        assert_eq!(m.ents[2].parent, Some(1));
    }

    #[test]
    fn notes() {
        let (m, _) = model("actor A\nnote right of A : hi\nnote \"x\" as N\nN .. A\nA --> (U)\nnote on link : l").unwrap();
        assert_eq!(m.ents.iter().filter(|e| matches!(e.def, Def::Note(_))).count(), 3);
    }

    #[test]
    fn errors() {
        assert!(model("actor A\nthis is junk").unwrap_err().starts_with("line 2: syntax error"));
        assert!(model("node X {\n[a]").unwrap_err().starts_with("line 1:"));
        assert!(model("}").unwrap_err().starts_with("line 1:"));
    }
}
