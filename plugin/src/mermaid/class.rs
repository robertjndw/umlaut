//! Mermaid class diagrams, filled into PlantUML's class model and drawn by its layout.
//!
//! Where Mermaid differs from PlantUML and the model has to be told:
//!
//! - Visibility characters are part of the member text and stay literal (`vis: None`),
//!   so no PlantUML visibility icons appear; there is no spot circle either.
//! - Annotations (`<<interface>>`) are only the stereotype line above the name. The
//!   class keeps `CKind::Class`, since the other kinds would italicize or recolor it.
//! - Generics are written into the name (`A<T>`) instead of PlantUML's dashed box.
//! - Both member compartments are drawn even when empty.
//!
//! Parsing is two-phase (see `mermaid/mod.rs`): statements build a draft of strings,
//! then labels are registered, since `classDef`, `style` and `cssClass` may come after
//! the class they style.

use std::collections::HashMap;

use super::css::{parse_assign, parse_style, split_classes, Classes, Css};
use super::lexer::{Dir, Header};
use super::state::id_len;
use super::{text, theme};
use crate::plantuml::diagrams::class::{CKind, Class, Def, Ent, Group, Item, Member, Model as PModel, Note, Show};
use crate::plantuml::diagrams::gcommon::{note_link, parse_arrow, Arrow, GroupStyle, Link, Look};
use crate::plantuml::layout::graph::Dir as GDir;
use crate::plantuml::layout::Body;
use crate::plantuml::lexer::{check_nesting, err, first_word, rest_after_word, Cursor, Line};
use crate::plantuml::model::{Ctx, LabelId, Measured};
use crate::plantuml::skin::Skin;
use crate::scene::Paint;

pub type Model = PModel;

enum Draft {
    Class {
        /// Name as shown, generics already in `~T~` form; the id when `None`.
        display: Option<String>,
        annots: Vec<String>,
        members: Vec<String>,
    },
    Namespace {
        display: Option<String>,
    },
    Note {
        text: String,
    },
}

struct DEnt {
    key: String,
    parent: Option<usize>,
    draft: Draft,
}

struct DLink {
    from: usize,
    to: usize,
    arrow: Arrow,
    label: Option<String>,
    from_card: Option<String>,
    to_card: Option<String>,
    line: usize,
}

struct P<'a> {
    ctx: &'a mut Ctx,
    ents: Vec<DEnt>,
    links: Vec<DLink>,
    /// Open namespaces as `(entity, line)`.
    scopes: Vec<(usize, usize)>,
    dir: Option<Dir>,
    classes: Classes,
    assigned: HashMap<String, Vec<String>>,
    styles: HashMap<String, Css>,
}

pub fn parse(lines: &[Line], header: &Header, ctx: &mut Ctx) -> Result<Model, String> {
    let mut p = P { ctx, ents: Vec::new(), links: Vec::new(), scopes: Vec::new(), dir: header.dir, classes: Classes::default(), assigned: HashMap::new(), styles: HashMap::new() };
    let mut cur = Cursor::new(lines);
    while let Some(l) = cur.next() {
        if !l.text.is_empty() {
            p.statement(l, &mut cur)?;
        }
    }
    if let Some((_, no)) = p.scopes.last() {
        return err(*no, "this namespace is never closed");
    }
    if p.ents.is_empty() {
        return err(header.line, "Mermaid class diagrams need at least one class");
    }
    Ok(p.register())
}

/// A class name at the start of `s`, with its `:::class` suffixes and what follows.
/// Names may be wrapped in backticks to hold other characters.
fn scan_name(s: &str) -> Option<(String, Vec<String>, &str)> {
    let s = s.trim_start();
    let (name, rest) = if let Some(q) = s.strip_prefix('`') {
        let end = q.find('`')?;
        (q[..end].to_string(), &q[end + 1..])
    } else {
        let n = id_len(s);
        if n == 0 {
            return None;
        }
        (s[..n].to_string(), &s[n..])
    };
    let mut classes = Vec::new();
    let mut rest = rest;
    while let Some(r) = rest.strip_prefix(":::") {
        let n = r.find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-')).unwrap_or(r.len());
        classes.push(r[..n].to_string());
        rest = &r[n..];
    }
    Some((name, classes, rest))
}

/// The relation token at the start of `s`: `<|--`, `..>`, `()--`, `--o` and so on.
fn scan_token(s: &str) -> (&str, &str) {
    let cs: Vec<(usize, char)> = s.char_indices().collect();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i].1;
        let prev = i.checked_sub(1).map(|j| cs[j].1);
        let next = cs.get(i + 1).map(|x| x.1);
        let ok = match c {
            '<' | '>' | '|' | '*' | '-' | '.' | '(' | ')' => true,
            'o' => (i == 0 && matches!(next, Some('-' | '.'))) || (matches!(prev, Some('-' | '.')) && next.is_none_or(|n| n.is_whitespace() || n == '"')),
            _ => false,
        };
        if !ok {
            break;
        }
        i += 1;
    }
    let end = cs.get(i).map_or(s.len(), |x| x.0);
    (&s[..end], &s[end..])
}

fn quoted(s: &str) -> Option<(&str, &str)> {
    let q = s.trim_start().strip_prefix('"')?;
    let end = q.find('"')?;
    Some((&q[..end], &q[end + 1..]))
}

struct Rel {
    left: (String, Vec<String>),
    right: (String, Vec<String>),
    left_card: Option<String>,
    right_card: Option<String>,
    arrow: Arrow,
    label: Option<String>,
}

/// `A "1" <|-- "*" B : label`. `None` when the line is not a relation.
fn parse_rel(s: &str) -> Option<Rel> {
    let (left, lc, rest) = scan_name(s)?;
    let mut rest = rest.trim_start();
    let mut left_card = None;
    if let Some((c, r)) = quoted(rest) {
        left_card = Some(c.to_string());
        rest = r.trim_start();
    }
    let (tok, rest) = scan_token(rest);
    if !tok.contains(['-', '.']) {
        return None;
    }
    let arrow = parse_arrow(tok).filter(|a| a.len >= 2)?;
    let mut rest = rest.trim_start();
    let mut right_card = None;
    if let Some((c, r)) = quoted(rest) {
        right_card = Some(c.to_string());
        rest = r.trim_start();
    }
    let (right, rc, rest) = scan_name(rest)?;
    let rest = rest.trim();
    let label = match rest.strip_prefix(':') {
        Some(t) => Some(t.trim().to_string()).filter(|t| !t.is_empty()),
        None if rest.is_empty() => None,
        None => return None,
    };
    Some(Rel { left: (left, lc), right: (right, rc), left_card, right_card, arrow, label })
}

fn looks_like_arrow(rest: &str) -> bool {
    !rest.starts_with("<<") && (rest.starts_with(['<', '-', '.', '*', '(']) || rest.starts_with("o-") || rest.starts_with("o."))
}

fn annotation(s: &str) -> Option<&str> {
    s.trim().strip_prefix("<<")?.strip_suffix(">>").map(str::trim)
}

impl P<'_> {
    fn scope_parent(&self) -> Option<usize> {
        self.scopes.last().map(|s| s.0)
    }

    fn find_class(&self, key: &str) -> Option<usize> {
        self.ents.iter().position(|e| e.key == key && matches!(e.draft, Draft::Class { .. }))
    }

    fn class(&mut self, key: &str) -> usize {
        if let Some(i) = self.find_class(key) {
            return i;
        }
        self.ents.push(DEnt { key: key.to_string(), parent: self.scope_parent(), draft: Draft::Class { display: None, annots: Vec::new(), members: Vec::new() } });
        self.ents.len() - 1
    }

    fn assign(&mut self, key: &str, classes: &[String]) {
        if !classes.is_empty() {
            self.assigned.entry(key.to_string()).or_default().extend(classes.iter().cloned());
        }
    }

    fn ignored(&mut self, l: &Line, what: &str) {
        self.ctx.warn(l.no, &format!("`{what}` is not supported and is ignored"));
    }

    fn statement(&mut self, l: &Line, cur: &mut Cursor) -> Result<(), String> {
        let text = l.text.as_str();
        if text == "}" {
            return match self.scopes.pop() {
                Some(_) => Ok(()),
                None => err(l.no, "`}` without a matching `{`"),
            };
        }
        let word = first_word(text);
        let rest = rest_after_word(text);
        if !looks_like_arrow(rest) {
            match word {
                "direction" => {
                    let Some(d) = Dir::parse(rest) else { return err(l.no, format!("unknown direction `{rest}`, use TB, BT, LR or RL")) };
                    self.dir = Some(d);
                    return Ok(());
                }
                "class" => return self.class_decl(l, rest, cur),
                "namespace" => return self.namespace(l, rest),
                "note" => return self.note(l, rest),
                "classDef" => {
                    self.classes.define(rest);
                    return Ok(());
                }
                "cssClass" => {
                    let Some((ids, names)) = parse_assign(rest) else { return err(l.no, format!("expected `cssClass \"<classes>\" <name>`, found \"{text}\"")) };
                    for id in ids {
                        self.assign(&id, &names);
                    }
                    return Ok(());
                }
                "style" => {
                    let Some((id, css)) = parse_style(rest) else { return err(l.no, format!("expected `style <class> <css>`, found \"{text}\"")) };
                    self.styles.entry(id).or_default().merge(&css);
                    return Ok(());
                }
                "click" | "link" | "callback" | "href" | "call" => {
                    self.ignored(l, word);
                    return Ok(());
                }
                _ => {}
            }
        }
        if text.starts_with("<<") {
            let Some(end) = text.find(">>") else { return err(l.no, "the annotation is missing its closing `>>`") };
            let annot = text[2..end].trim().to_string();
            let Some((name, classes, tail)) = scan_name(&text[end + 2..]) else { return err(l.no, format!("expected a class name after `<<{annot}>>`")) };
            if !tail.trim().is_empty() {
                return err(l.no, format!("cannot understand \"{}\" after the class name", tail.trim()));
            }
            let idx = self.class(&name);
            self.assign(&name, &classes);
            self.annotate(idx, annot);
            return Ok(());
        }
        if let Some(rel) = parse_rel(text) {
            let from = self.class(&rel.left.0);
            self.assign(&rel.left.0, &rel.left.1);
            let to = self.class(&rel.right.0);
            self.assign(&rel.right.0, &rel.right.1);
            self.links.push(DLink { from, to, arrow: rel.arrow, label: rel.label, from_card: rel.left_card, to_card: rel.right_card, line: l.no });
            return Ok(());
        }
        if let Some((name, classes, rest)) = scan_name(text) {
            let rest = rest.trim_start();
            if rest.is_empty() {
                self.class(&name);
                self.assign(&name, &classes);
                return Ok(());
            }
            if let Some(member) = rest.strip_prefix(':') {
                let idx = self.class(&name);
                self.assign(&name, &classes);
                self.member(idx, member.trim());
                return Ok(());
            }
        }
        err(l.no, format!("syntax error, cannot understand \"{text}\""))
    }

    fn annotate(&mut self, idx: usize, annot: String) {
        if let Draft::Class { annots, .. } = &mut self.ents[idx].draft {
            annots.push(annot);
        }
    }

    fn member(&mut self, idx: usize, text: &str) {
        if text.is_empty() {
            return;
        }
        if let Some(a) = annotation(text) {
            self.annotate(idx, a.to_string());
        } else if let Draft::Class { members, .. } = &mut self.ents[idx].draft {
            members.push(text.to_string());
        }
    }

    /// `class A`, `class A["Label"]`, `class A~T~`, `class A { ... }`, `class A:::c`.
    fn class_decl(&mut self, l: &Line, rest: &str, cur: &mut Cursor) -> Result<(), String> {
        let (rest, body) = match rest.strip_suffix("{}") {
            Some(r) => (r.trim(), false),
            None => match rest.strip_suffix('{') {
                Some(r) => (r.trim(), true),
                None => (rest, false),
            },
        };
        let Some((name, mut classes, mut tail)) = scan_name(rest) else { return err(l.no, format!("expected a class name, found \"{rest}\"")) };
        let mut generic = None;
        if let Some(r) = tail.strip_prefix('~') {
            let Some(end) = r.rfind('~') else { return err(l.no, "the generic type is missing its closing `~`") };
            generic = Some(r[..end].to_string());
            tail = &r[end + 1..];
        }
        let mut label = None;
        if let Some(r) = tail.trim_start().strip_prefix('[') {
            let Some(end) = r.rfind(']') else { return err(l.no, "the class label is missing its closing `]`") };
            label = Some(r[..end].trim().trim_matches(['"', '`']).to_string());
            tail = &r[end + 1..];
        }
        let (tail, more) = split_classes(tail.trim());
        classes.extend(more);
        if !tail.is_empty() {
            return err(l.no, format!("cannot understand \"{tail}\" after the class name"));
        }
        let idx = self.class(&name);
        // A class mentioned in a relation before its namespace declares it moves in.
        if let (Some(scope), None) = (self.scope_parent(), self.ents[idx].parent) {
            self.ents[idx].parent = Some(scope);
        }
        self.assign(&name, &classes);
        let display = label.or_else(|| generic.map(|g| format!("{name}~{g}~")));
        if let (Draft::Class { display: d, .. }, Some(display)) = (&mut self.ents[idx].draft, display) {
            *d = Some(display);
        }
        if body {
            loop {
                let Some(n) = cur.next() else { return err(l.no, format!("the body of `{name}` is never closed")) };
                if n.text == "}" {
                    break;
                }
                self.member(idx, &n.text);
            }
        }
        Ok(())
    }

    fn namespace(&mut self, l: &Line, rest: &str) -> Result<(), String> {
        let Some(head) = rest.strip_suffix('{') else { return err(l.no, "expected `namespace <name> {`") };
        let head = head.trim();
        let (name, display) = match head.split_once('[') {
            Some((n, lab)) => (n.trim(), Some(lab.trim_end_matches(']').trim().trim_matches(['"', '`']).to_string())),
            None => (head, None),
        };
        if name.is_empty() {
            return err(l.no, "the namespace needs a name");
        }
        check_nesting(self.scopes.len(), l.no)?;
        let idx = match self.ents.iter().position(|e| e.key == name && matches!(e.draft, Draft::Namespace { .. })) {
            Some(i) => i,
            None => {
                self.ents.push(DEnt { key: name.to_string(), parent: self.scope_parent(), draft: Draft::Namespace { display } });
                self.ents.len() - 1
            }
        };
        self.scopes.push((idx, l.no));
        Ok(())
    }

    /// `note "text"` and `note for A "text"`.
    fn note(&mut self, l: &Line, rest: &str) -> Result<(), String> {
        let (target, tail) = match rest.strip_prefix("for ") {
            Some(r) => {
                let Some((name, _, tail)) = scan_name(r) else { return err(l.no, "expected a class name after `note for`") };
                (Some(name), tail)
            }
            None => (None, rest),
        };
        let Some((body, after)) = quoted(tail) else { return err(l.no, "expected the note's text in double quotes") };
        if !after.trim().is_empty() {
            return err(l.no, format!("cannot understand \"{}\" after the note's text", after.trim()));
        }
        let body = body.replace("\\n", "\n");
        if body.trim().is_empty() {
            return err(l.no, "the note is empty");
        }
        let class = target.map(|t| self.class(&t));
        let parent = class.and_then(|c| self.ents[c].parent).or_else(|| self.scope_parent());
        self.ents.push(DEnt { key: String::new(), parent, draft: Draft::Note { text: body } });
        if let Some(c) = class {
            // Links are registered after entities, so this one has no label to order.
            let note = self.ents.len() - 1;
            let link = note_link(note, c, GDir::Right, l.no);
            self.links.push(DLink { from: note, to: c, arrow: link.arrow, label: None, from_card: None, to_card: None, line: l.no });
        }
        Ok(())
    }

    /// Phase two: styles are known, so labels can be registered in their fixed order.
    fn register(self) -> Model {
        let P { ctx, ents, links, dir, classes, assigned, styles, .. } = self;
        let n = ents.len();
        let mut defs: Vec<Def> = Vec::with_capacity(n);
        let mut parents = Vec::with_capacity(n);
        let mut keys = Vec::with_capacity(n);
        for e in ents {
            parents.push(e.parent);
            let def = match e.draft {
                Draft::Namespace { display } => {
                    let base = theme::cluster(&ctx.skin);
                    let label = ctx.reg.add_opt(text::runs(display.as_deref().unwrap_or(&e.key), &base), None);
                    let css = classes.resolve(assigned.get(&e.key).map_or(&[][..], |v| v), styles.get(&e.key));
                    let mut look = css.look();
                    look.fill.get_or_insert_with(|| Paint::solid(theme::CLUSTER_FILL));
                    look.line.get_or_insert_with(|| crate::scene::hex(theme::BORDER2));
                    Def::Group(Group { label, style: GroupStyle::Rectangle, look })
                }
                Draft::Note { text: t } => {
                    let label = ctx.reg.add(text::runs(&t, &theme::note(&ctx.skin)), None);
                    Def::Note(Note { label, look: Look::default(), on_link: None })
                }
                Draft::Class { display, annots, members } => {
                    let css = classes.resolve(assigned.get(&e.key).map_or(&[][..], |v| v), styles.get(&e.key));
                    let mut name_base = theme::class_name(&ctx.skin).bold();
                    let mut member_base = theme::class_member(&ctx.skin);
                    for b in [&mut name_base, &mut member_base] {
                        if let Some(c) = &css.color {
                            b.color = crate::scene::hex(c);
                        }
                        b.italic |= css.italic;
                    }
                    member_base.bold |= css.bold;
                    let name = ctx.reg.add(text::plain_runs(display.as_deref().unwrap_or(&e.key), &name_base), None);
                    let stereo_text: Vec<String> = annots.iter().map(|a| format!("\u{ab}{a}\u{bb}")).collect();
                    let stereo = (!stereo_text.is_empty()).then(|| ctx.reg.add(text::plain_runs(&stereo_text.join("\n"), &member_base), None));
                    let items = members.iter().map(|m| Item::Member(member(ctx, m, &member_base))).collect();
                    Def::Class(Class {
                        kind: CKind::Class,
                        name,
                        generics: None,
                        stereo_text: annots,
                        stereo,
                        spot: None,
                        items,
                        look: css.look(),
                        show: Show { circle: false, fields: true, methods: true, stereo: true, empty_fields: true, empty_methods: true },
                    })
                }
            };
            defs.push(def);
            keys.push(e.key);
        }
        let edge = theme::edge(&ctx.skin);
        let mut out_links: Vec<Link> = links
            .into_iter()
            .map(|d| {
                let mut lab = |t: Option<String>| t.and_then(|t| ctx.reg.add_opt(text::runs(&t, &edge), None));
                let label = lab(d.label);
                let from_card = lab(d.from_card);
                let to_card = lab(d.to_card);
                Link { from: d.from, to: d.to, arrow: d.arrow, label, label_dir: None, from_card, to_card, line: d.line }
            })
            .collect();

        // The layout reads a parent's slot while walking the list, so parents must come
        // first. A class that moved into a later namespace breaks that order.
        let mut order = Vec::with_capacity(n);
        let mut placed = vec![false; n];
        fn place(i: usize, parents: &[Option<usize>], placed: &mut [bool], order: &mut Vec<usize>) {
            if placed[i] {
                return;
            }
            placed[i] = true;
            if let Some(p) = parents[i] {
                place(p, parents, placed, order);
            }
            order.push(i);
        }
        for i in 0..n {
            place(i, &parents, &mut placed, &mut order);
        }
        let mut new_index = vec![0; n];
        for (new, &old) in order.iter().enumerate() {
            new_index[old] = new;
        }
        let mut defs: Vec<Option<Def>> = defs.into_iter().map(Some).collect();
        let ents = order
            .iter()
            .map(|&old| Ent { key: std::mem::take(&mut keys[old]), parent: parents[old].map(|p| new_index[p]), def: defs[old].take().expect("each entity is placed once") })
            .collect();
        for l in &mut out_links {
            l.from = new_index[l.from];
            l.to = new_index[l.to];
        }
        let (lr, reverse) = dir.map_or((false, false), |d| (d.lr(), d.reverse()));
        Model { ents, links: out_links, lr, reverse }
    }
}

/// One member line. A trailing `$` makes it static (underlined) and a trailing `*`
/// abstract (italic); both come after the return type of a method.
fn member(ctx: &mut Ctx, raw: &str, base: &crate::plantuml::creole::Base) -> Member {
    let t = raw.trim();
    let (t, stat, abs) = if let Some(r) = t.strip_suffix('$') {
        (r.trim_end(), true, false)
    } else if let Some(r) = t.strip_suffix('*') {
        (r.trim_end(), false, true)
    } else {
        (t, false, false)
    };
    let mut runs = text::plain_runs(t, base);
    for r in &mut runs {
        r.underline |= stat;
        r.italic |= abs;
    }
    let label: LabelId = ctx.reg.add(runs, None);
    Member { vis: None, label, method: t.contains('(') }
}

pub fn layout(m: &Model, skin: &Skin, meas: &Measured) -> Result<Body, String> {
    crate::plantuml::layout::class::layout(m, skin, meas)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plantuml::creole;
    use crate::plantuml::diagrams::gcommon::Deco;

    fn model(src: &str) -> Result<(Model, Ctx), String> {
        let mut f = super::super::front(format!("classDiagram\n{src}").as_bytes())?;
        let m = parse(&f.lines, &f.header, &mut f.ctx)?;
        Ok((m, f.ctx))
    }

    fn class<'a>(m: &'a Model, key: &str) -> &'a Class {
        match &m.ents.iter().find(|e| e.key == key).unwrap().def {
            Def::Class(c) => c,
            _ => panic!("{key} is not a class"),
        }
    }

    fn plain(ctx: &Ctx, id: LabelId) -> String {
        creole::plain(&ctx.reg.labels[id.0].runs)
    }

    fn members(ctx: &Ctx, c: &Class) -> Vec<(String, bool)> {
        c.items.iter().filter_map(|i| if let Item::Member(m) = i { Some((plain(ctx, m.label), m.method)) } else { None }).collect()
    }

    #[test]
    fn declarations_labels_and_generics() {
        let (m, ctx) = model("class A\nclass B[\"Bee label\"]\nclass C~T~\nclass D~List~int~~").unwrap();
        assert_eq!(plain(&ctx, class(&m, "A").name), "A");
        assert_eq!(plain(&ctx, class(&m, "B").name), "Bee label");
        assert_eq!(plain(&ctx, class(&m, "C").name), "C<T>");
        assert_eq!(plain(&ctx, class(&m, "D").name), "D<List<int>>");
        assert!(class(&m, "C").generics.is_none());
        assert!(!class(&m, "A").show.circle && class(&m, "A").show.empty_methods);
    }

    #[test]
    fn members_in_bodies_and_lines() {
        let (m, ctx) = model("class A {\n  +String name\n  -int age$\n  +run() bool\n  +stop()*\n  +List~int~ ids\n}\nA : +extra(x) int").unwrap();
        let c = class(&m, "A");
        assert_eq!(
            members(&ctx, c),
            [("+String name".to_string(), false), ("-int age".to_string(), false), ("+run() bool".to_string(), true), ("+stop()".to_string(), true), ("+List<int> ids".to_string(), false), ("+extra(x) int".to_string(), true)]
        );
        let Item::Member(age) = &c.items[1] else { panic!() };
        assert!(ctx.reg.labels[age.label.0].runs[0].underline);
        let Item::Member(stop) = &c.items[3] else { panic!() };
        assert!(ctx.reg.labels[stop.label.0].runs[0].italic);
        assert!(c.items.iter().all(|i| matches!(i, Item::Member(m) if m.vis.is_none())));
    }

    #[test]
    fn annotations_in_three_places() {
        let (m, ctx) = model("class A {\n  <<interface>>\n  +f()\n}\n<<abstract>> B\nclass C\nC : <<service>>").unwrap();
        assert_eq!(plain(&ctx, class(&m, "A").stereo.unwrap()), "\u{ab}interface\u{bb}");
        assert_eq!(plain(&ctx, class(&m, "B").stereo.unwrap()), "\u{ab}abstract\u{bb}");
        assert_eq!(class(&m, "C").stereo_text, ["service"]);
        assert_eq!(class(&m, "A").kind, CKind::Class);
    }

    #[test]
    fn relations() {
        let (m, ctx) = model("A <|-- B\nA *-- C\nA o-- D\nA --> E\nA -- F\nA ..> G\nA ..|> H\nA .. I\nA <|.. J\nA <--> K\nA ()-- L\nA --() M\nA \"1\" --> \"*\" N : has\nO --o P").unwrap();
        let ends = |i: usize| (m.links[i].arrow.tail, m.links[i].arrow.head, m.links[i].arrow.dashed);
        assert_eq!(ends(0), (Deco::Triangle, Deco::None, false));
        assert_eq!(ends(1), (Deco::DiamondFilled, Deco::None, false));
        assert_eq!(ends(2), (Deco::Diamond, Deco::None, false));
        assert_eq!(ends(3), (Deco::None, Deco::Arrow, false));
        assert_eq!(ends(4), (Deco::None, Deco::None, false));
        assert_eq!(ends(5), (Deco::None, Deco::Arrow, true));
        assert_eq!(ends(6), (Deco::None, Deco::Triangle, true));
        assert_eq!(ends(7), (Deco::None, Deco::None, true));
        assert_eq!(ends(8), (Deco::Triangle, Deco::None, true));
        assert_eq!(ends(9), (Deco::Arrow, Deco::Arrow, false));
        assert_eq!(ends(10).0, Deco::Lollipop);
        assert_eq!(ends(11).1, Deco::Lollipop);
        assert_eq!(ends(13).1, Deco::Diamond);
        assert!(m.links.iter().all(|l| l.arrow.len >= 2));
        let l = &m.links[12];
        assert_eq!((plain(&ctx, l.from_card.unwrap()), plain(&ctx, l.to_card.unwrap()), plain(&ctx, l.label.unwrap())), ("1".into(), "*".into(), "has".into()));
        // The tail is the first class: the parent of `A <|-- B` is A.
        assert_eq!(m.ents[m.links[0].from].key, "A");
    }

    #[test]
    fn compact_relations_and_dashed_names() {
        let (m, _) = model("Animal<|--Dog\nmy-class --> other_class : uses").unwrap();
        assert_eq!(m.ents[m.links[0].to].key, "Dog");
        assert!(m.ents.iter().any(|e| e.key == "my-class"));
    }

    #[test]
    fn namespaces_own_their_classes() {
        let (m, ctx) = model("A --> B\nnamespace Core {\n  class A\n  class C\n}\nnamespace Ext[\"External\"] {\n  class D\n}").unwrap();
        let ns = m.ents.iter().position(|e| e.key == "Core").unwrap();
        assert!(ns < m.ents.iter().position(|e| e.key == "A").unwrap());
        let a = m.ents.iter().position(|e| e.key == "A").unwrap();
        assert_eq!(m.ents[a].parent, Some(ns));
        assert_eq!(m.ents[m.links[0].from].key, "A");
        assert_eq!(m.ents[m.links[0].to].key, "B");
        let Def::Group(g) = &m.ents[m.ents.iter().position(|e| e.key == "Ext").unwrap()].def else { panic!() };
        assert_eq!(plain(&ctx, g.label.unwrap()), "External");
        assert!(model("namespace X {\nclass A").unwrap_err().starts_with("line 2:"));
    }

    #[test]
    fn notes() {
        let (m, ctx) = model("class A\nnote \"free\\nfloating\"\nnote for A \"about A\"").unwrap();
        let notes: Vec<_> = m.ents.iter().filter_map(|e| if let Def::Note(n) = &e.def { Some(n) } else { None }).collect();
        assert_eq!(plain(&ctx, notes[0].label), "free\nfloating");
        assert_eq!(m.links.len(), 1);
        assert!(m.links[0].arrow.dashed);
        assert!(model("class A\nnote for A text").is_err());
    }

    #[test]
    fn styles_come_before_or_after() {
        let (m, ctx) = model("class A:::hot\nclass B\nclassDef hot fill:#f00,color:#fff\ncssClass \"B\" hot\nstyle B stroke:#00f").unwrap();
        for k in ["A", "B"] {
            let c = class(&m, k);
            assert_eq!(c.look.fill.as_ref().unwrap().color, "#ff0000ff");
            assert_eq!(ctx.reg.labels[c.name.0].runs[0].color, "#ffffffff");
        }
        assert_eq!(class(&m, "B").look.line.as_deref(), Some("#0000ffff"));
    }

    #[test]
    fn directions() {
        let (m, _) = model("direction LR\nA --> B").unwrap();
        assert!(m.lr && !m.reverse);
        let (m, _) = model("direction BT\nA --> B").unwrap();
        assert!(!m.lr && m.reverse);
    }

    #[test]
    fn registration_order_is_stable() {
        let (_, ctx) = model("A <|-- B : r\nA : +x").unwrap();
        let texts: Vec<String> = ctx.reg.labels.iter().map(|l| creole::plain(&l.runs)).collect();
        assert_eq!(texts, ["A", "+x", "B", "r"]);
    }

    #[test]
    fn ignored_and_errors() {
        let (_, ctx) = model("class A\nclick A href \"x\"\nlink A \"y\"\ncallback A \"cb\"").unwrap();
        assert_eq!(ctx.warnings.len(), 3);
        assert!(model("class A\nnonsense here").unwrap_err().starts_with("line 3: syntax error"));
        assert!(model("class A {\n+x").unwrap_err().contains("never closed"));
        assert!(model("}").unwrap_err().starts_with("line 2:"));
        assert!(model("A -> B").unwrap_err().starts_with("line 2:"));
        assert!(model("").unwrap_err().contains("Mermaid class diagrams"));
    }
}
