//! Class diagrams: classes and friends with their members, packages, links.
//!
//! Entities and packages live in one list so a link can end on either; classes are
//! created on first mention (in a link, a member line or a note), and a later
//! declaration fills in what the first mention could not know.

use super::gcommon::*;
use crate::plantuml::creole;
use crate::plantuml::layout::graph::RankDir;
use crate::plantuml::lexer::{err, find_outside_quotes, first_word, rest_after_word, unquote, Cursor, Line};
use crate::plantuml::model::{Ctx, LabelId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CKind {
    Class,
    Abstract,
    Interface,
    Enum,
    Annotation,
    Entity,
    Protocol,
    Struct,
    Exception,
    Metaclass,
    Stereotype,
}

impl CKind {
    fn from_word(w: &str) -> Option<CKind> {
        Some(match w {
            "class" => CKind::Class,
            "abstract" => CKind::Abstract,
            "interface" => CKind::Interface,
            "enum" => CKind::Enum,
            "annotation" => CKind::Annotation,
            "entity" => CKind::Entity,
            "protocol" => CKind::Protocol,
            "struct" => CKind::Struct,
            "exception" => CKind::Exception,
            "metaclass" => CKind::Metaclass,
            "stereotype" => CKind::Stereotype,
            _ => return None,
        })
    }

    /// The letter in the header's circle.
    pub fn letter(self) -> char {
        match self {
            CKind::Class => 'C',
            CKind::Abstract => 'A',
            CKind::Interface => 'I',
            CKind::Enum => 'E',
            CKind::Annotation => '@',
            CKind::Entity => 'E',
            CKind::Protocol => 'P',
            CKind::Struct => 'S',
            CKind::Exception => 'E',
            CKind::Metaclass => 'M',
            CKind::Stereotype => 'S',
        }
    }

    /// Fill of that circle, as PlantUML draws it.
    pub fn circle_fill(self) -> &'static str {
        match self {
            CKind::Class => "#ADD1B2",
            CKind::Abstract => "#A9DCDF",
            CKind::Interface => "#B4A7E5",
            CKind::Enum => "#EB937F",
            CKind::Annotation => "#E3664A",
            CKind::Entity => "#ADD1B2",
            CKind::Protocol => "#ADD1B2",
            CKind::Struct => "#ADD1B2",
            CKind::Exception => "#ADD1B2",
            CKind::Metaclass => "#ADD1B2",
            CKind::Stereotype => "#ADD1B2",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Member {
    /// `-`, `#`, `~` or `+`.
    pub vis: Option<char>,
    pub label: LabelId,
    pub method: bool,
}

#[derive(Debug, Clone)]
pub enum Item {
    Member(Member),
    /// `--`, `..`, `==` or `__` with an optional title.
    Sep { style: char, title: Option<LabelId> },
}

/// What `hide` and `show` decide for one class.
#[derive(Debug, Clone, Copy)]
pub struct Show {
    pub circle: bool,
    pub fields: bool,
    pub methods: bool,
    pub stereo: bool,
    pub empty_fields: bool,
    pub empty_methods: bool,
}

#[derive(Debug, Clone)]
pub struct Class {
    pub kind: CKind,
    pub name: LabelId,
    pub generics: Option<LabelId>,
    pub stereo_text: Vec<String>,
    pub stereo: Option<LabelId>,
    pub spot: Option<(char, Option<String>)>,
    pub items: Vec<Item>,
    pub look: Look,
    pub show: Show,
}

#[derive(Debug, Clone)]
pub struct Group {
    pub label: Option<LabelId>,
    pub style: GroupStyle,
    pub look: Look,
}

pub use super::gcommon::Note;

#[derive(Debug, Clone)]
pub enum Def {
    Class(Class),
    Group(Group),
    Note(Note),
}

pub type Ent = super::gcommon::Ent<Def>;

#[derive(Debug)]
pub struct Model {
    pub ents: Vec<Ent>,
    pub links: Vec<Link>,
    pub dir: RankDir,
}

#[derive(Clone, Copy, PartialEq)]
enum What {
    Circle,
    Members,
    Fields,
    Methods,
    Stereotype,
    EmptyMembers,
    EmptyFields,
    EmptyMethods,
}

enum Target {
    All,
    Kind(CKind),
    Stereo(String),
    Key(String),
}

struct Rule {
    target: Target,
    what: What,
    show: bool,
}

enum Scope {
    Group(usize, usize),
    Together(usize),
}

struct P<'a> {
    ctx: &'a mut Ctx,
    ents: Vec<Ent>,
    links: Vec<Link>,
    scopes: Vec<Scope>,
    rules: Vec<Rule>,
    last: Option<usize>,
    dir: RankDir,
}

pub fn parse(lines: &[Line], ctx: &mut Ctx) -> Result<Model, String> {
    let mut p = P { ctx, ents: Vec::new(), links: Vec::new(), scopes: Vec::new(), rules: Vec::new(), last: None, dir: RankDir::TB };
    let mut cur = Cursor::new(lines);
    while let Some(l) = cur.next() {
        if !l.text.is_empty() {
            p.statement(l, &mut cur)?;
        }
    }
    if let Some(s) = p.scopes.last() {
        let (Scope::Group(_, no) | Scope::Together(no)) = s;
        return err(*no, "this block is never closed");
    }
    if p.ents.is_empty() {
        return err(lines.first().map_or(1, |l| l.no), "the class diagram is empty");
    }
    p.apply_rules();
    Ok(Model { ents: p.ents, links: p.links, dir: p.dir })
}

/// Position of `pat` that is not inside `<...>`, so `Foo<T extends Bar>` is not an `extends`.
fn find_outside_angles(s: &str, pat: &str) -> Option<usize> {
    let mut depth = 0;
    for (i, c) in s.char_indices() {
        match c {
            '<' => depth += 1,
            '>' => depth = (depth - 1).max(0),
            _ => {}
        }
        if depth == 0 && s[i..].starts_with(pat) {
            return Some(i);
        }
    }
    None
}

/// `Name<T>` into the name and its generic parameter.
fn split_generics(s: &str) -> (&str, Option<&str>) {
    if let Some(i) = s.find('<') {
        if !s[i..].starts_with("<<") && s.ends_with('>') {
            return (s[..i].trim(), Some(s[i + 1..s.len() - 1].trim()));
        }
    }
    (s, None)
}

impl P<'_> {
    fn scope_parent(&self) -> Option<usize> {
        self.scopes.iter().rev().find_map(|s| match s {
            Scope::Group(g, _) => Some(*g),
            Scope::Together(_) => None,
        })
    }

    fn name_base(&self, kind: CKind) -> creole::Base {
        let b = self.ctx.skin.text(&["class"], 14.0, "#000000");
        if kind == CKind::Abstract { b.italic() } else { b }
    }

    fn member_base(&self) -> creole::Base {
        self.ctx.skin.text(&["classattribute", "class"], 14.0, "#000000")
    }

    fn arrow_base(&self) -> creole::Base {
        self.ctx.skin.text(&["classarrow", "arrow"], 13.0, "#000000")
    }

    fn label(&mut self, text: &str, base: &creole::Base) -> LabelId {
        let runs = creole::runs(text, base);
        self.ctx.reg.add(runs, None)
    }

    fn new_class(&mut self, key: &str, display: &str, kind: CKind) -> usize {
        let base = self.name_base(kind);
        let (name, generics) = split_generics(display);
        let name = self.label(name, &base);
        let generics = generics.map(|g| {
            let b = self.ctx.skin.text(&["class"], 12.0, "#000000");
            self.label(g, &b)
        });
        let class = Class {
            kind,
            name,
            generics,
            stereo_text: Vec::new(),
            stereo: None,
            spot: None,
            items: Vec::new(),
            look: Look::default(),
            show: Show { circle: true, fields: true, methods: true, stereo: true, empty_fields: true, empty_methods: true },
        };
        self.ents.push(Ent { key: key.to_string(), parent: self.scope_parent(), def: Def::Class(class) });
        let idx = self.ents.len() - 1;
        self.last = Some(idx);
        idx
    }

    fn find(&self, key: &str) -> Option<usize> {
        self.ents.iter().position(|e| e.key == key && !key.is_empty())
    }

    fn resolve(&mut self, r: &Ref, line: usize) -> Result<usize, String> {
        let name = match r {
            Ref::Name(n) | Ref::Bracket(n) | Ref::Paren(n) | Ref::Colon(n) => n.as_str(),
            Ref::Interface(Some(n)) => n.as_str(),
            Ref::Interface(None) | Ref::Star => return err(line, "this reference is not valid in a class diagram"),
        };
        if let Some(i) = self.find(name) {
            return Ok(i);
        }
        let kind = if matches!(r, Ref::Interface(_)) { CKind::Interface } else { CKind::Class };
        Ok(self.new_class(name, name, kind))
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
            "together {" | "together{" => {
                crate::plantuml::lexer::check_nesting(self.scopes.len(), l.no)?;
                self.scopes.push(Scope::Together(l.no));
                return Ok(());
            }
            _ => {}
        }
        match word {
            "hide" | "show" => return self.hide_show(l, word == "show"),
            "set" | "allow_mixing" | "allowmixing" | "remove" | "restore" => {
                self.ctx.warn(l.no, &format!("`{word}` is not supported and is ignored"));
                return Ok(());
            }
            "note" | "floating" => {
                if let Some(n) = parse_note(l, cur)? {
                    return self.note(l, n);
                }
            }
            _ => {}
        }
        if let Some(r) = self.group_head(l)? {
            return Ok(r);
        }
        if let Some(r) = self.class_decl(l, cur)? {
            return Ok(r);
        }
        if let Some(parts) = parse_link(text) {
            return self.link(l, parts);
        }
        if let Some((r, rest)) = scan_ref(text) {
            if let Some(member) = rest.trim_start().strip_prefix(':') {
                if !matches!(r, Ref::Star) {
                    let idx = self.resolve(&r, l.no)?;
                    self.last = Some(idx);
                    return self.member(l, idx, member.trim());
                }
            }
        }
        err(l.no, format!("syntax error, cannot understand \"{text}\""))
    }

    // ---- groups --------------------------------------------------------------------------

    /// `package name <<Style>> #color {` and the other containers.
    fn group_head(&mut self, l: &Line) -> Result<Option<()>, String> {
        let text = l.text.as_str();
        let lower = text.to_ascii_lowercase();
        let word = first_word(&lower);
        if !matches!(word, "package" | "namespace" | "rectangle" | "folder" | "frame" | "node" | "cloud" | "database") {
            return Ok(None);
        }
        let rest = rest_after_word(text);
        let Some(head) = rest.strip_suffix('{') else { return Ok(None) };
        let (head, stereos) = take_stereos(head);
        let (head, look) = take_look(&head);
        let mut style = GroupStyle::from_word(word).unwrap_or(GroupStyle::Package);
        if let Some(s) = stereos.iter().find_map(|s| GroupStyle::from_word(&s.text)) {
            style = s;
        }
        let (display, key) = match find_outside_quotes(head, ' ').and_then(|_| head.rfind(" as ")) {
            Some(i) => (unquote(&head[..i]).to_string(), head[i + 4..].trim().to_string()),
            None => (unquote(head).to_string(), unquote(head).to_string()),
        };
        let base = self.ctx.skin.text(&["package"], 14.0, "#000000");
        let label = if display.is_empty() { None } else { self.ctx.reg.add_opt(creole::runs(&display, &base), None) };
        // Reopening a package adds to it instead of making a second one.
        let idx = match self.ents.iter().position(|e| e.key == key && matches!(e.def, Def::Group(_))) {
            Some(i) => i,
            None => {
                self.ents.push(Ent { key, parent: self.scope_parent(), def: Def::Group(Group { label, style, look: look.clone() }) });
                self.ents.len() - 1
            }
        };
        crate::plantuml::lexer::check_nesting(self.scopes.len(), l.no)?;
        self.scopes.push(Scope::Group(idx, l.no));
        Ok(Some(()))
    }

    // ---- classes -------------------------------------------------------------------------

    fn class_decl(&mut self, l: &Line, cur: &mut Cursor) -> Result<Option<()>, String> {
        let text = l.text.as_str();
        let lower = text.to_ascii_lowercase();
        let word = first_word(&lower);
        let mut rest = rest_after_word(text);
        let mut kind = match CKind::from_word(word) {
            Some(k) => k,
            None => return Ok(None),
        };
        if word == "abstract" && first_word(&rest.to_ascii_lowercase()) == "class" {
            rest = rest_after_word(rest);
        }
        // `class` followed by `<`/`:` or an arrow is not a declaration (a class named
        // like a keyword in a link, say).
        if rest.is_empty() || rest.starts_with(':') || (parse_link(text).is_some() && !rest.starts_with('"')) {
            return Ok(None);
        }
        let (mut head, body) = match rest.strip_suffix("{}") {
            Some(h) => (h.trim(), false),
            None => match rest.strip_suffix('{') {
                Some(h) => (h.trim(), true),
                None => (rest, false),
            },
        };
        let (h, stereos) = take_stereos(head);
        let (h, look) = take_look(&h);
        let mut h = h.to_string();
        // `extends` and `implements`.
        let mut parents: Vec<(String, bool)> = Vec::new();
        for (kw, is_impl) in [(" implements ", true), (" extends ", false)] {
            if let Some(i) = find_outside_angles(&h.to_ascii_lowercase(), kw) {
                let names = h[i + kw.len()..].to_string();
                h.truncate(i);
                for n in names.split(',') {
                    let n = n.trim();
                    if !n.is_empty() {
                        parents.push((unquote(n).to_string(), is_impl));
                    }
                }
            }
        }
        // Names after `implements` may themselves hold ` extends ...`; good enough.
        head = h.trim();
        let (display, key) = if let Some(q) = head.strip_prefix('"') {
            let end = q.find('"').ok_or_else(|| format!("line {}: the class name is missing its closing quote", l.no))?;
            let after = q[end + 1..].trim();
            let key = match after.strip_prefix("as ") {
                Some(a) => a.trim().to_string(),
                None if after.is_empty() => q[..end].to_string(),
                None => return err(l.no, format!("cannot understand \"{after}\" after the class name")),
            };
            (q[..end].to_string(), key)
        } else if let Some(i) = head.find(" as ") {
            (head[..i].trim().to_string(), head[i + 4..].trim().to_string())
        } else {
            (head.to_string(), split_generics(head).0.to_string())
        };
        if key.is_empty() || key.contains(char::is_whitespace) {
            return err(l.no, format!("cannot understand the class name \"{head}\""));
        }
        let idx = match self.find(&key) {
            Some(i) => {
                // A mention made the class exist before; the declaration decides its kind.
                let base = self.name_base(kind);
                let (name, generics) = split_generics(&display);
                let name = self.label(name, &base);
                let g = generics.map(|g| {
                    let b = self.ctx.skin.text(&["class"], 12.0, "#000000");
                    self.label(g, &b)
                });
                if let Def::Class(c) = &mut self.ents[i].def {
                    c.kind = kind;
                    c.name = name;
                    if g.is_some() {
                        c.generics = g;
                    }
                } else {
                    return err(l.no, format!("`{key}` is already used for a package"));
                }
                i
            }
            None => self.new_class(&key, &display, kind),
        };
        self.last = Some(idx);
        if kind == CKind::Class && stereos.iter().any(|s| s.text.eq_ignore_ascii_case("abstract")) {
            kind = CKind::Abstract;
        }
        let stereo_labels: Vec<String> = stereos.iter().filter(|s| !s.text.is_empty()).map(|s| s.text.clone()).collect();
        let spot = stereos.iter().find_map(|s| s.spot.clone());
        let stereo = if stereo_labels.is_empty() {
            None
        } else {
            let base = self.ctx.skin.text(&["classstereotype", "stereotype"], 12.0, "#000000").italic();
            let text = stereo_labels.iter().map(|s| format!("\u{ab}{s}\u{bb}")).collect::<Vec<_>>().join("\n");
            Some(self.label(&text, &base))
        };
        if let Def::Class(c) = &mut self.ents[idx].def {
            c.kind = kind;
            if stereo.is_some() {
                c.stereo = stereo;
                c.stereo_text = stereo_labels;
            }
            if spot.is_some() {
                c.spot = spot;
            }
            if look != Look::default() {
                c.look = look;
            }
        }
        for (p, is_impl) in parents {
            let parent = match self.find(&p) {
                Some(i) => i,
                None => self.new_class(&p, &p, if is_impl { CKind::Interface } else { CKind::Class }),
            };
            self.last = Some(idx);
            let mut arrow = Arrow::plain(2);
            arrow.tail = Deco::Triangle;
            arrow.dashed = is_impl;
            self.links.push(Link { from: parent, to: idx, arrow, label: None, label_dir: None, from_card: None, to_card: None, line: l.no });
        }
        if body {
            loop {
                let Some(n) = cur.next() else { return err(l.no, format!("the body of `{key}` is never closed")) };
                if n.text == "}" {
                    break;
                }
                if !n.text.is_empty() {
                    self.member(n, idx, &n.text)?;
                }
            }
        }
        Ok(Some(()))
    }

    fn member(&mut self, l: &Line, idx: usize, text: &str) -> Result<(), String> {
        let t = text.trim();
        if t.is_empty() {
            return Ok(());
        }
        if !matches!(self.ents[idx].def, Def::Class(_)) {
            return err(l.no, "members can only be added to classes");
        }
        let cs: Vec<char> = t.chars().collect();
        // `--`, `..`, `==` and `__` start a separator, optionally with a title between two of them.
        if cs.len() >= 2 && matches!(cs[0], '-' | '.' | '=' | '_') && cs[1] == cs[0] {
            let style = cs[0];
            let pair: String = [style, style].iter().collect();
            let inner = t.trim_start_matches(style);
            let whole = inner.is_empty();
            let titled = inner.trim_end().ends_with(&pair);
            if whole || titled {
                let title = inner.trim().trim_end_matches(style).trim();
                let id = if title.is_empty() {
                    None
                } else {
                    let base = self.member_base();
                    self.ctx.reg.add_opt(creole::runs(title, &base), None)
                };
                if let Def::Class(c) = &mut self.ents[idx].def {
                    c.items.push(Item::Sep { style, title: id });
                }
                return Ok(());
            }
        }
        let mut s = t;
        let (mut stat, mut abs, mut forced): (bool, bool, Option<bool>) = (false, false, None);
        let mut vis = None;
        // Modifiers and the visibility marker come in either order: `{static} -x`, `+{static} x`.
        for round in 0..2 {
            loop {
                let lw = s.to_ascii_lowercase();
                let Some((m, f)) = [("{static}", 0), ("{classifier}", 0), ("{abstract}", 1), ("{field}", 2), ("{method}", 3)].into_iter().find(|(m, _)| lw.starts_with(m)) else { break };
                match f {
                    0 => stat = true,
                    1 => abs = true,
                    2 => forced = Some(false),
                    _ => forced = Some(true),
                }
                s = s[m.len()..].trim_start();
            }
            if round == 0 {
                if let Some(ch) = s.chars().next() {
                    if matches!(ch, '-' | '#' | '~' | '+') && !s.starts_with("--") {
                        vis = Some(ch);
                        s = s[1..].trim_start();
                    }
                }
            }
        }
        let method = forced.unwrap_or_else(|| s.contains('('));
        let base = self.member_base();
        let mut runs = creole::runs(s, &base);
        for r in &mut runs {
            if stat {
                r.underline = true;
            }
            if abs {
                r.italic = true;
            }
        }
        let label = self.ctx.reg.add(runs, None);
        if let Def::Class(c) = &mut self.ents[idx].def {
            c.items.push(Item::Member(Member { vis, label, method }));
        }
        Ok(())
    }

    // ---- links and notes ---------------------------------------------------------------------

    fn link(&mut self, l: &Line, p: LinkParts) -> Result<(), String> {
        let from = self.resolve(&p.left, l.no)?;
        let to = self.resolve(&p.right, l.no)?;
        let base = self.arrow_base();
        let label = |this: &mut Self, t: &str| this.ctx.reg.add_opt(creole::runs(t, &base), None);
        let lab = p.label.as_deref().and_then(|t| label(self, t));
        let fc = p.left_card.as_deref().and_then(|t| label(self, t));
        let tc = p.right_card.as_deref().and_then(|t| label(self, t));
        self.links.push(Link { from, to, arrow: p.arrow, label: lab, label_dir: p.label_dir, from_card: fc, to_card: tc, line: l.no });
        Ok(())
    }

    fn note(&mut self, l: &Line, n: NoteSyntax) -> Result<(), String> {
        let base = self.ctx.skin.text(&["note"], 13.0, "#000000");
        let Some(label) = self.ctx.reg.add_opt(creole::runs(&n.text, &base), None) else { return err(l.no, "the note is empty") };
        let key = n.alias.clone().unwrap_or_default();
        let (target, on_link) = note_target(&n.place, self.last, self.links.len(), l.no, |r| self.resolve(r, l.no))?;
        let parent = target.and_then(|t| self.ents[t].parent).or_else(|| self.scope_parent());
        self.ents.push(Ent { key, parent, def: Def::Note(Note { label, look: n.look, on_link }) });
        if let Some(t) = target {
            self.links.push(note_link(self.ents.len() - 1, t, n.side, l.no));
        }
        Ok(())
    }

    // ---- hide and show ---------------------------------------------------------------------

    fn hide_show(&mut self, l: &Line, show: bool) -> Result<(), String> {
        let lower = l.text.to_ascii_lowercase();
        let mut rest = rest_after_word(&lower).trim().to_string();
        let what = [
            ("empty members", What::EmptyMembers),
            ("empty fields", What::EmptyFields),
            ("empty attributes", What::EmptyFields),
            ("empty methods", What::EmptyMethods),
            ("members", What::Members),
            ("attributes", What::Fields),
            ("fields", What::Fields),
            ("methods", What::Methods),
            ("circle", What::Circle),
            ("stereotype", What::Stereotype),
        ]
        .iter()
        .find(|(w, _)| rest == *w || rest.ends_with(&format!(" {w}")))
        .map(|&(w, k)| (w, k));
        let Some((w, kind)) = what else {
            self.ctx.warn(l.no, &format!("`{}` is not supported and is ignored", l.text));
            return Ok(());
        };
        rest.truncate(rest.len() - w.len());
        let t = rest.trim();
        // Keys are case sensitive, so read the target from the original text.
        let orig = l.text[l.text.len() - lower.len()..].to_string();
        let orig_rest = rest_after_word(&orig).trim();
        let t_orig = orig_rest[..t.len().min(orig_rest.len())].trim();
        let target = if t.is_empty() {
            Target::All
        } else if let Some(s) = t_orig.strip_prefix("<<").and_then(|x| x.strip_suffix(">>")) {
            Target::Stereo(s.trim().to_string())
        } else if let Some(k) = CKind::from_word(t) {
            Target::Kind(k)
        } else {
            Target::Key(unquote(t_orig).to_string())
        };
        if kind == What::Members {
            for x in [What::Fields, What::Methods] {
                self.rules.push(Rule { target: clone_target(&target), what: x, show });
            }
        } else if kind == What::EmptyMembers {
            for x in [What::EmptyFields, What::EmptyMethods] {
                self.rules.push(Rule { target: clone_target(&target), what: x, show });
            }
        } else {
            self.rules.push(Rule { target, what: kind, show });
        }
        Ok(())
    }

    fn apply_rules(&mut self) {
        for r in &self.rules {
            for e in &mut self.ents {
                let Def::Class(c) = &mut e.def else { continue };
                let hit = match &r.target {
                    Target::All => true,
                    Target::Kind(k) => c.kind == *k,
                    Target::Stereo(s) => c.stereo_text.iter().any(|t| t.eq_ignore_ascii_case(s)),
                    Target::Key(k) => &e.key == k,
                };
                if hit {
                    match r.what {
                        What::Circle => c.show.circle = r.show,
                        What::Fields => c.show.fields = r.show,
                        What::Methods => c.show.methods = r.show,
                        What::Stereotype => c.show.stereo = r.show,
                        What::EmptyFields => c.show.empty_fields = r.show,
                        What::EmptyMethods => c.show.empty_methods = r.show,
                        What::Members | What::EmptyMembers => {}
                    }
                }
            }
        }
    }
}

fn clone_target(t: &Target) -> Target {
    match t {
        Target::All => Target::All,
        Target::Kind(k) => Target::Kind(*k),
        Target::Stereo(s) => Target::Stereo(s.clone()),
        Target::Key(s) => Target::Key(s.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plantuml::layout::graph::Dir;

    fn model(src: &str) -> Result<(Model, Ctx), String> {
        crate::plantuml::parse_with(src, parse)
    }

    fn class(m: &Model, key: &str) -> Class {
        match &m.ents.iter().find(|e| e.key == key).unwrap().def {
            Def::Class(c) => c.clone(),
            _ => panic!("not a class"),
        }
    }

    fn text(ctx: &Ctx, id: LabelId) -> String {
        creole::plain(&ctx.reg.labels[id.0].runs)
    }

    #[test]
    fn declarations() {
        let (m, ctx) = model("abstract class A <<Serializable>> #pink\ninterface \"Long Name\" as I\nenum E {\n  RED\n  GREEN\n}\nclass Foo<T>\nannotation An").unwrap();
        assert_eq!(class(&m, "A").kind, CKind::Abstract);
        assert_eq!(class(&m, "A").stereo_text, ["Serializable"]);
        assert!(class(&m, "A").look.fill.is_some());
        assert_eq!(text(&ctx, class(&m, "I").name), "Long Name");
        assert_eq!(class(&m, "E").items.len(), 2);
        let foo = class(&m, "Foo");
        assert_eq!(text(&ctx, foo.name), "Foo");
        assert_eq!(text(&ctx, foo.generics.unwrap()), "T");
        assert_eq!(class(&m, "An").kind, CKind::Annotation);
    }

    #[test]
    fn members_fields_methods_and_modifiers() {
        let (m, ctx) = model("class A {\n  -int x\n  #name : String\n  ~pkg()\n  +{static} create() : A\n  {abstract} run()\n  --\n  .. title ..\n  plain\n}").unwrap();
        let c = class(&m, "A");
        let members: Vec<&Member> = c.items.iter().filter_map(|i| if let Item::Member(m) = i { Some(m) } else { None }).collect();
        assert_eq!(members.iter().map(|m| m.vis).collect::<Vec<_>>(), [Some('-'), Some('#'), Some('~'), Some('+'), None, None]);
        assert_eq!(members.iter().map(|m| m.method).collect::<Vec<_>>(), [false, false, true, true, true, false]);
        assert!(ctx.reg.labels[members[3].label.0].runs.iter().all(|r| r.underline));
        assert!(ctx.reg.labels[members[4].label.0].runs.iter().all(|r| r.italic));
        assert_eq!(c.items.iter().filter(|i| matches!(i, Item::Sep { .. })).count(), 2);
    }

    #[test]
    fn member_lines_outside_the_body() {
        let (m, _) = model("class A\nA : +x : int\nA : run()\nclass B").unwrap();
        assert_eq!(class(&m, "A").items.len(), 2);
    }

    #[test]
    fn links_create_classes_and_keep_decorations() {
        let (m, ctx) = model("A <|-- B\nB \"1\" *-- \"many\" C : has >\nC ..> D\nD --|> A : x").unwrap();
        assert_eq!(m.ents.len(), 4);
        assert_eq!(m.links.len(), 4);
        assert_eq!(m.links[0].arrow.tail, Deco::Triangle);
        assert_eq!(text(&ctx, m.links[1].from_card.unwrap()), "1");
        assert_eq!(m.links[1].label_dir, Some(true));
        assert!(m.links[2].arrow.dashed);
    }

    #[test]
    fn packages_nest() {
        let (m, _) = model("package outer {\n  package inner <<Folder>> {\n    class A\n  }\n  class B\n}\nclass C").unwrap();
        let idx = |k: &str| m.ents.iter().position(|e| e.key == k).unwrap();
        assert_eq!(m.ents[idx("A")].parent, Some(idx("inner")));
        assert_eq!(m.ents[idx("inner")].parent, Some(idx("outer")));
        assert_eq!(m.ents[idx("B")].parent, Some(idx("outer")));
        assert_eq!(m.ents[idx("C")].parent, None);
        let Def::Group(g) = &m.ents[idx("inner")].def else { panic!() };
        assert_eq!(g.style, GroupStyle::Folder);
    }

    #[test]
    fn notes_attach_and_float() {
        let (m, _) = model("class A\nnote left of A : hi\nnote \"x\" as N1\nN1 .. A\nA --> B\nnote on link : y").unwrap();
        let notes = m.ents.iter().filter(|e| matches!(e.def, Def::Note(_))).count();
        assert_eq!(notes, 3);
        let Def::Note(n) = &m.ents.last().unwrap().def else { panic!() };
        assert_eq!(n.on_link, Some(m.links.len() - 1));
        assert_eq!(m.links[0].arrow.dir, Some(Dir::Right));
    }

    #[test]
    fn hide_and_show_rules() {
        let (m, _) = model("class A\nclass B <<x>>\nhide empty members\nhide circle\nshow B circle\nhide <<x>> methods\nhide A fields").unwrap();
        let a = class(&m, "A");
        assert!(!a.show.circle && !a.show.empty_fields && !a.show.empty_methods && !a.show.fields);
        let b = class(&m, "B");
        assert!(b.show.circle && !b.show.methods && b.show.fields);
    }

    #[test]
    fn generics_may_contain_extends() {
        let (m, ctx) = model("class Foo<T extends Bar>").unwrap();
        assert_eq!(m.links.len(), 0);
        let Def::Class(c) = &m.ents[0].def else { panic!() };
        assert_eq!(text(&ctx, c.generics.unwrap()), "T extends Bar");
    }

    #[test]
    fn extends_and_implements() {
        let (m, _) = model("class A extends B implements C, D").unwrap();
        assert_eq!(m.links.len(), 3);
        assert!(m.links[0].arrow.dashed && m.links[1].arrow.dashed && !m.links[2].arrow.dashed);
    }

    #[test]
    fn errors_carry_line_numbers() {
        assert!(model("class A\nthis is junk").unwrap_err().starts_with("line 2: syntax error"));
        assert!(model("package p {\nclass A").unwrap_err().starts_with("line 1:"));
        assert!(model("class A {\n+x").unwrap_err().starts_with("line 1:"));
        assert!(model("}").unwrap_err().starts_with("line 1:"));
        assert!(model("").unwrap_err().contains("empty"));
    }

    #[test]
    fn direction_directive() {
        assert!(model("left to right direction\nclass A").unwrap().0.dir.lr());
    }
}
