//! Flowchart source to `Model`. A statement is first read into a `Chain` (vertices and
//! the arrows between them) without touching any state, then applied to the `Parser`.
//! `Parser::finish` is phase two of the two-phase rule in `mermaid/mod.rs`: styles are
//! resolved and labels registered only once the whole source has been read.

use std::collections::HashMap;

use super::{Def, Model, Node, Shape, Subgraph};
use crate::mermaid::css::{parse_assign, parse_style, Classes, Css};
use crate::mermaid::lexer::{parse_dir, Header};
use crate::mermaid::{text, theme};
use crate::plantuml::diagrams::gcommon::{Arrow, Deco, Ent, Link};
use crate::plantuml::lexer::{err, first_word, rest_after_word, unquote, Line};
use crate::plantuml::model::Ctx;

/// Edge widths. Mermaid draws normal edges 2 px and thick ones 3.5 px next to 1 px node
/// borders; the picture is scaled with the page, so the ratios are what matter.
const NORMAL_WIDTH: f32 = 1.5;
const THICK_WIDTH: f32 = 3.0;

#[derive(Clone, Copy, PartialEq)]
enum Family {
    Solid,
    Thick,
    Dotted,
}

/// One arrow as written, before its endpoints are known.
#[derive(Debug, Clone, PartialEq)]
struct Tok {
    tail: Deco,
    head: Deco,
    dashed: bool,
    thick: bool,
    hidden: bool,
    /// Ranks the arrow spans: 2 for `-->`, one more per extra body character.
    len: usize,
    label: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct Vertex {
    id: String,
    shaped: Option<(Shape, String)>,
    classes: Vec<String>,
    /// `e1@{ animate: true }`: properties of an edge, not a node.
    edge_props: bool,
    /// A v11 `shape:` name that is drawn as a rectangle.
    unsupported: Option<String>,
}

#[derive(Debug, PartialEq)]
struct Chain {
    groups: Vec<Vec<Vertex>>,
    edges: Vec<Tok>,
}

struct Sc {
    c: Vec<char>,
    i: usize,
    no: usize,
}

fn id_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '.' | '$' | '%' | '*' | '+' | '!' | '?' | '\'')
}

/// Openers longest first; each has the closers that end it, and the shape each closer
/// gives (the slash and backslash forms share an opener).
const OPENERS: &[(&str, &[(&str, Shape)])] = &[
    ("(((", &[(")))", Shape::DoubleCircle)]),
    ("((", &[("))", Shape::Circle)]),
    ("([", &[("])", Shape::Stadium)]),
    ("(", &[(")", Shape::Round)]),
    ("[[", &[("]]", Shape::Subroutine)]),
    ("[(", &[(")]", Shape::Cylinder)]),
    ("[/", &[("/]", Shape::LeanRight), ("\\]", Shape::Trapezoid)]),
    ("[\\", &[("\\]", Shape::LeanLeft), ("/]", Shape::TrapezoidAlt)]),
    ("[", &[("]", Shape::Rect)]),
    (">", &[("]", Shape::Asymmetric)]),
    ("{{", &[("}}", Shape::Hexagon)]),
    ("{", &[("}", Shape::Rhombus)]),
];

impl Sc {
    fn new(text: &str, no: usize) -> Sc {
        Sc { c: text.chars().collect(), i: 0, no }
    }

    fn peek(&self) -> Option<char> {
        self.c.get(self.i).copied()
    }

    fn at(&self, k: usize) -> Option<char> {
        self.c.get(self.i + k).copied()
    }

    fn starts(&self, s: &str) -> bool {
        s.chars().enumerate().all(|(k, ch)| self.at(k) == Some(ch))
    }

    fn ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.i += 1;
        }
    }

    fn run(&mut self, ch: char) -> usize {
        let start = self.i;
        while self.peek() == Some(ch) {
            self.i += 1;
        }
        self.i - start
    }

    fn rest(&self) -> String {
        self.c[self.i..].iter().collect()
    }

    fn fail<T>(&self, msg: impl std::fmt::Display) -> Result<T, String> {
        err(self.no, msg)
    }

    /// Skips a quoted string starting at the cursor.
    fn skip_quoted(&mut self) -> Result<(), String> {
        self.i += 1;
        while let Some(c) = self.peek() {
            self.i += 1;
            if c == '"' {
                return Ok(());
            }
        }
        self.fail("unclosed quote")
    }

    fn ident(&mut self) -> String {
        let start = self.i;
        loop {
            match self.peek() {
                Some(c) if id_char(c) => self.i += 1,
                // A dash belongs to the id unless it begins an arrow.
                Some('-') if !matches!(self.at(1), Some('-' | '.' | '>')) => self.i += 1,
                _ => break,
            }
        }
        self.c[start..self.i].iter().collect()
    }

    /// Text up to one of the closers, skipping quoted strings. Returns it with the index
    /// of the closer that ended it and leaves the cursor behind the closer.
    fn until(&mut self, closers: &[&str], what: &str) -> Result<(String, usize), String> {
        let start = self.i;
        while self.peek().is_some() {
            if self.peek() == Some('"') {
                self.skip_quoted()?;
                continue;
            }
            if let Some(k) = closers.iter().position(|c| self.starts(c)) {
                let text = self.c[start..self.i].iter().collect();
                self.i += closers[k].chars().count();
                return Ok((text, k));
            }
            self.i += 1;
        }
        self.fail(format!("unclosed {what}"))
    }

    fn shaped(&mut self) -> Result<Option<(Shape, String)>, String> {
        for (open, closers) in OPENERS {
            if self.starts(open) {
                self.i += open.chars().count();
                let names: Vec<&str> = closers.iter().map(|c| c.0).collect();
                let (text, k) = self.until(&names, &format!("`{open}`"))?;
                return Ok(Some((closers[k].1, text)));
            }
        }
        Ok(None)
    }

    fn vertex(&mut self) -> Result<Vertex, String> {
        let id = self.ident();
        if id.is_empty() {
            return match self.peek() {
                Some(c) => self.fail(format!("expected a node id but found `{}`", c)),
                None => self.fail("expected a node id after the arrow"),
            };
        }
        if id == "end" {
            return self.fail("`end` closes a subgraph, so it cannot be a node id; write `End` or `END` instead");
        }
        let mut v = Vertex { id, shaped: None, classes: Vec::new(), edge_props: false, unsupported: None };
        if self.starts("@{") {
            self.i += 2;
            let (body, _) = self.until(&["}"], "`@{`")?;
            let props = parse_props(&body);
            let has = |k: &str| props.iter().any(|(n, _)| n == k);
            if !has("shape") && !has("label") && (has("animate") || has("animation") || has("curve")) {
                v.edge_props = true;
            } else {
                let name = props.iter().find(|(n, _)| n == "shape").map(|(_, s)| s.trim().trim_matches('"').to_ascii_lowercase());
                let shape = match name.as_deref().map(|n| (n, shape_by_name(n))) {
                    None => Shape::Rect,
                    Some((_, Some(s))) => s,
                    Some((n, None)) => {
                        v.unsupported = Some(n.to_string());
                        Shape::Rect
                    }
                };
                let label = props.iter().find(|(n, _)| n == "label").map_or_else(|| v.id.clone(), |(_, l)| l.clone());
                v.shaped = Some((shape, label));
            }
        } else {
            v.shaped = self.shaped()?;
        }
        if self.starts(":::") {
            let start = self.i;
            while self.peek().is_some_and(|c| !c.is_whitespace() && c != '&') {
                self.i += 1;
            }
            let suffix: String = self.c[start..self.i].iter().collect();
            v.classes = suffix.split(":::").map(|c| c.trim().to_string()).filter(|c| !c.is_empty()).collect();
        }
        Ok(v)
    }

    fn group(&mut self) -> Result<Vec<Vertex>, String> {
        let mut g = vec![self.vertex()?];
        loop {
            self.ws();
            if self.peek() != Some('&') {
                return Ok(g);
            }
            self.i += 1;
            self.ws();
            g.push(self.vertex()?);
        }
    }

    /// An optional `id@` before an arrow (v11 edge ids).
    fn skip_edge_id(&mut self) {
        let mut k = 0;
        while self.at(k).is_some_and(|c| c.is_alphanumeric() || c == '_') {
            k += 1;
        }
        if k > 0 && self.at(k) == Some('@') && matches!(self.at(k + 1), Some('-' | '=' | '~' | '<' | 'o' | 'x')) {
            self.i += k + 1;
        }
    }

    /// Where a label text ends. `fam` decides what the closing arrow looks like.
    /// Returns the label with the head and number of body characters of that arrow.
    fn label_and_end(&mut self, fam: Family) -> Result<(String, Deco, usize), String> {
        let start = self.i;
        while self.peek().is_some() {
            if self.peek() == Some('"') {
                self.skip_quoted()?;
                continue;
            }
            let here = self.i;
            if let Some((head, n)) = self.end_token(fam) {
                let text = self.c[start..here].iter().collect::<String>();
                return Ok((text.trim().to_string(), head, n));
            }
            self.i = here + 1;
        }
        self.fail("an edge label is missing the arrow that ends it")
    }

    /// The closing arrow of a `-- text -->` edge at the cursor, consumed on a match.
    fn end_token(&mut self, fam: Family) -> Option<(Deco, usize)> {
        let at = self.i;
        let found = match fam {
            Family::Dotted => {
                let dots = self.run('.');
                (dots >= 1 && self.peek() == Some('-')).then(|| {
                    self.i += 1;
                    (self.terminator().unwrap_or(Deco::None), dots + 1)
                })
            }
            Family::Solid | Family::Thick => {
                let ch = if fam == Family::Solid { '-' } else { '=' };
                let d = self.run(ch);
                match (d >= 2).then(|| self.terminator()).flatten() {
                    Some(head) => Some((head, d)),
                    None if d >= 3 => Some((Deco::None, d - 1)),
                    None => None,
                }
            }
        };
        if found.is_none() {
            self.i = at;
        }
        found
    }

    fn terminator(&mut self) -> Option<Deco> {
        let head = match self.peek()? {
            '>' => Deco::Point,
            'x' => Deco::Cross,
            'o' => Deco::Circle,
            _ => return None,
        };
        self.i += 1;
        Some(head)
    }

    /// `|text|` after an arrow.
    fn pipe_label(&mut self) -> Result<Option<String>, String> {
        let at = self.i;
        self.ws();
        if self.peek() != Some('|') {
            self.i = at;
            return Ok(None);
        }
        self.i += 1;
        let (text, _) = self.until(&["|"], "`|`")?;
        Ok(Some(text.trim().to_string()))
    }

    /// The next arrow, or `None` when the statement goes on with something else.
    fn edge(&mut self) -> Result<Option<Tok>, String> {
        self.ws();
        self.skip_edge_id();
        let start = self.i;
        let mut tok = Tok { tail: Deco::None, head: Deco::None, dashed: false, thick: false, hidden: false, len: 2, label: None };
        match self.peek() {
            Some('<') => {
                tok.tail = Deco::Point;
                self.i += 1;
            }
            Some(c @ ('o' | 'x')) if matches!(self.at(1), Some('-' | '=')) => {
                tok.tail = if c == 'o' { Deco::Circle } else { Deco::Cross };
                self.i += 1;
            }
            _ => {}
        }
        let fam = match (self.peek(), self.at(1)) {
            (Some('-'), Some('.')) => Family::Dotted,
            (Some('-'), Some('-')) => Family::Solid,
            (Some('='), Some('=')) => Family::Thick,
            (Some('~'), Some('~')) if self.starts("~~~") => {
                let n = self.run('~');
                tok.hidden = true;
                tok.len = 2 + (n - 3);
                tok.label = self.pipe_label()?;
                return Ok(Some(tok));
            }
            _ => {
                self.i = start;
                return Ok(None);
            }
        };
        tok.thick = fam == Family::Thick;
        tok.dashed = fam == Family::Dotted;
        let n = match fam {
            Family::Dotted => {
                self.i += 1;
                let dots = self.run('.');
                if self.peek() == Some('-') {
                    self.i += 1;
                    tok.head = self.terminator().unwrap_or(Deco::None);
                    dots + 1
                } else {
                    let (label, head, n) = self.label_and_end(fam)?;
                    tok.label = Some(label);
                    tok.head = head;
                    n
                }
            }
            Family::Solid | Family::Thick => {
                let ch = if fam == Family::Solid { '-' } else { '=' };
                let d = self.run(ch);
                if let Some(head) = self.terminator() {
                    tok.head = head;
                    d
                } else if d >= 3 {
                    d - 1
                } else {
                    let (label, head, n) = self.label_and_end(fam)?;
                    tok.label = Some(label);
                    tok.head = head;
                    n
                }
            }
        };
        tok.len = 2 + n.saturating_sub(2);
        if tok.label.is_none() {
            tok.label = self.pipe_label()?;
        }
        Ok(Some(tok))
    }

    fn chain(&mut self) -> Result<Chain, String> {
        let mut groups = vec![self.group()?];
        let mut edges = Vec::new();
        loop {
            self.ws();
            if self.peek().is_none() {
                return Ok(Chain { groups, edges });
            }
            let Some(tok) = self.edge()? else {
                return self.fail(format!("cannot read `{}`; expected an arrow such as `-->` here", self.rest()));
            };
            edges.push(tok);
            self.ws();
            groups.push(self.group()?);
        }
    }
}

/// `key: value` pairs of an `@{ ... }` block, separated by commas or newlines outside
/// quotes. Values keep their quotes, so the label path can strip or read them as markdown.
fn parse_props(body: &str) -> Vec<(String, String)> {
    let mut pieces = vec![String::new()];
    let mut quoted = false;
    for c in body.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                pieces.last_mut().unwrap().push(c);
            }
            ',' | '\n' if !quoted => pieces.push(String::new()),
            _ => pieces.last_mut().unwrap().push(c),
        }
    }
    pieces
        .iter()
        .filter_map(|p| p.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .filter(|(k, _)| !k.is_empty())
        .collect()
}

fn shape_by_name(name: &str) -> Option<Shape> {
    Some(match name {
        "rect" | "rectangle" | "proc" | "process" | "div-rect" | "div-proc" | "divided-rectangle" | "divided-process" => Shape::Rect,
        "rounded" | "event" => Shape::Round,
        "stadium" | "terminal" | "pill" => Shape::Stadium,
        "subproc" | "subprocess" | "subroutine" | "fr-rect" | "framed-rectangle" => Shape::Subroutine,
        "cyl" | "cylinder" | "database" | "db" => Shape::Cylinder,
        "circle" | "circ" => Shape::Circle,
        "dbl-circ" | "double-circle" => Shape::DoubleCircle,
        "odd" => Shape::Asymmetric,
        "diam" | "diamond" | "decision" | "question" => Shape::Rhombus,
        "hex" | "hexagon" | "prepare" => Shape::Hexagon,
        "lean-r" | "lean-right" | "in-out" => Shape::LeanRight,
        "lean-l" | "lean-left" | "out-in" => Shape::LeanLeft,
        "trap-b" | "trapezoid-bottom" | "priority" | "trapezoid" => Shape::Trapezoid,
        "trap-t" | "trapezoid-top" | "inv-trapezoid" | "manual" => Shape::TrapezoidAlt,
        _ => return None,
    })
}

// ---- parser state ---------------------------------------------------------------------

enum Kind {
    Node { shape: Shape, text: Option<String> },
    Sub { title: Option<String> },
}

struct PEnt {
    key: String,
    kind: Kind,
    parent: Option<usize>,
    classes: Vec<String>,
    own: Css,
}

struct PLink {
    from: usize,
    to: usize,
    tok: Tok,
    line: usize,
}

struct Open {
    ent: usize,
    line: usize,
    /// Everything mentioned directly inside, in order.
    mentions: Vec<usize>,
}

struct Parser<'a> {
    ctx: &'a mut Ctx,
    ents: Vec<PEnt>,
    index: HashMap<String, usize>,
    links: Vec<PLink>,
    stack: Vec<Open>,
    classes: Classes,
    styles: Vec<(usize, String, Css)>,
    assigns: Vec<(usize, Vec<String>, Vec<String>)>,
    link_default: Css,
    link_styles: Vec<(usize, usize, Css)>,
}

impl Parser<'_> {
    /// The entity called `key`, created as `kind` if it is new, and counted as mentioned
    /// in the innermost open subgraph.
    fn intern(&mut self, key: &str, kind: impl FnOnce() -> Kind) -> usize {
        let idx = match self.index.get(key) {
            Some(&i) => i,
            None => {
                self.ents.push(PEnt { key: key.to_string(), kind: kind(), parent: None, classes: Vec::new(), own: Css::default() });
                self.index.insert(key.to_string(), self.ents.len() - 1);
                self.ents.len() - 1
            }
        };
        if let Some(top) = self.stack.last_mut() {
            top.mentions.push(idx);
        }
        idx
    }

    fn touch(&mut self, v: &Vertex) -> usize {
        let idx = self.intern(&v.id, || Kind::Node { shape: Shape::Rect, text: None });
        if let (Some((shape, label)), Kind::Node { shape: s, text }) = (&v.shaped, &mut self.ents[idx].kind) {
            *s = *shape;
            *text = Some(label.clone());
        }
        self.ents[idx].classes.extend(v.classes.iter().cloned());
        idx
    }

    fn statement(&mut self, line: &Line) -> Result<(), String> {
        let no = line.no;
        let text = line.text.trim();
        let (word, rest) = (first_word(text), rest_after_word(text));
        match word {
            "subgraph" => return self.open(rest, no),
            "end" if rest.is_empty() => return self.close(no),
            "direction" if parse_dir(rest).is_some() => {
                self.ctx.warn(no, "`direction` is ignored; the whole diagram flows in the direction of the header");
                return Ok(());
            }
            "classDef" if !rest.is_empty() => {
                self.classes.define(rest);
                return Ok(());
            }
            "class" if !rest.is_empty() => {
                let Some((ids, names)) = parse_assign(rest) else { return err(no, "`class` needs node ids and a class name, as in `class a,b highlight`") };
                self.assigns.push((no, ids, names));
                return Ok(());
            }
            "style" if !rest.is_empty() => {
                let Some((id, css)) = parse_style(rest) else { return err(no, "`style` needs a node id and its properties, as in `style A fill:#f9f`") };
                self.styles.push((no, id, css));
                return Ok(());
            }
            "linkStyle" if !rest.is_empty() => return self.link_style(rest, no),
            "click" | "call" | "href" if !rest.is_empty() => {
                self.ctx.warn(no, &format!("`{word}` is ignored (diagrams are static)"));
                return Ok(());
            }
            _ => {}
        }
        let chain = Sc::new(text, no).chain()?;
        if chain.groups.len() == 1 && chain.groups[0].len() == 1 && chain.groups[0][0].edge_props {
            self.ctx.warn(no, "edge properties such as `animate` are ignored");
            return Ok(());
        }
        let mut ids: Vec<Vec<usize>> = Vec::new();
        for g in &chain.groups {
            let mut one = Vec::new();
            for v in g {
                if let Some(name) = &v.unsupported {
                    self.ctx.warn(no, &format!("shape `{name}` is not supported and `{}` is drawn as a rectangle", v.id));
                }
                one.push(self.touch(v));
            }
            ids.push(one);
        }
        for (k, tok) in chain.edges.iter().enumerate() {
            for &from in &ids[k] {
                for &to in &ids[k + 1] {
                    self.links.push(PLink { from, to, tok: tok.clone(), line: no });
                }
            }
        }
        Ok(())
    }

    fn open(&mut self, rest: &str, no: usize) -> Result<(), String> {
        if rest.is_empty() {
            return err(no, "`subgraph` needs a name, as in `subgraph id [Title]`");
        }
        let mut sc = Sc::new(rest, no);
        let id = sc.ident();
        let tail = sc.rest();
        let tail = tail.trim();
        let (key, title) = if !id.is_empty() && tail.is_empty() {
            (id.clone(), Some(id))
        } else if !id.is_empty() && tail.starts_with('[') && tail.ends_with(']') {
            (id, Some(tail[1..tail.len() - 1].trim().to_string()).filter(|t| !t.is_empty()))
        } else {
            let t = unquote(rest).to_string();
            (t.clone(), Some(t))
        };
        // A subgraph mentioned in an arrow before its `subgraph` line is already a plain
        // node; it turns into the cluster in place, so those arrows keep their ends.
        let idx = self.intern(&key, || Kind::Sub { title: None });
        self.ents[idx].kind = Kind::Sub { title };
        self.stack.push(Open { ent: idx, line: no, mentions: Vec::new() });
        Ok(())
    }

    /// Mermaid lets the first subgraph to close claim a node that several mention, so a
    /// node mentioned in a nested subgraph and in its parent ends up in the nested one.
    fn close(&mut self, no: usize) -> Result<(), String> {
        let Some(done) = self.stack.pop() else { return err(no, "`end` without a matching `subgraph`") };
        for m in done.mentions {
            let is_open = m == done.ent || self.stack.iter().any(|o| o.ent == m);
            if !is_open && self.ents[m].parent.is_none() {
                self.ents[m].parent = Some(done.ent);
            }
        }
        Ok(())
    }

    fn link_style(&mut self, rest: &str, no: usize) -> Result<(), String> {
        let (targets, decl) = (first_word(rest), rest_after_word(rest));
        let css = Css::parse(decl);
        if targets == "default" {
            self.link_default.merge(&css);
            return Ok(());
        }
        for t in targets.split(',') {
            let Ok(n) = t.trim().parse::<usize>() else { return err(no, format!("`linkStyle` expects link numbers or `default`, not `{}`", t.trim())) };
            self.link_styles.push((no, n, css.clone()));
        }
        Ok(())
    }

    fn finish(mut self, header: &Header) -> Result<Model, String> {
        if let Some(open) = self.stack.last() {
            return err(open.line, format!("subgraph `{}` is never closed with `end`", self.ents[open.ent].key));
        }
        if self.ents.is_empty() {
            return err(header.line, "Mermaid flowcharts need at least one node");
        }
        for (no, id, css) in std::mem::take(&mut self.styles) {
            match self.index.get(&id) {
                Some(&i) => self.ents[i].own.merge(&css),
                None => self.ctx.warn(no, &format!("`style` refers to `{id}`, which is not in the diagram")),
            }
        }
        for (no, ids, names) in std::mem::take(&mut self.assigns) {
            for id in ids {
                match self.index.get(&id) {
                    Some(&i) => self.ents[i].classes.extend(names.iter().cloned()),
                    None => self.ctx.warn(no, &format!("`class` refers to `{id}`, which is not in the diagram")),
                }
            }
        }
        let n_links = self.links.len();
        for &(no, n, _) in &self.link_styles {
            if n >= n_links {
                self.ctx.warn(no, &format!("`linkStyle` refers to link {n}, but the diagram has {n_links} links"));
            }
        }
        let mut link_css = vec![self.link_default.clone(); n_links];
        for (_, n, c) in &self.link_styles {
            if let Some(css) = link_css.get_mut(*n) {
                css.merge(c);
            }
        }

        let skin = &self.ctx.skin;
        let node_base = theme::node(skin);
        let cluster_base = theme::cluster(skin);
        let edge_base = theme::edge(skin);

        let mut ents = Vec::new();
        for e in &self.ents {
            let def = match &e.kind {
                Kind::Node { shape, text: t } => {
                    let css = self.classes.resolve(&e.classes, Some(&e.own));
                    let src = t.as_deref().unwrap_or(&e.key);
                    let label = self.ctx.reg.add_opt(text::runs(src, &css.text(&node_base)), None);
                    Def::Node(Node { label, shape: *shape, css })
                }
                Kind::Sub { title } => {
                    let css = self.classes.resolve_without_default(&e.classes, Some(&e.own));
                    let title = title.as_ref().and_then(|t| self.ctx.reg.add_opt(text::runs(t, &css.text(&cluster_base)), None));
                    Def::Subgraph(Subgraph { title, css })
                }
            };
            ents.push(Ent { key: e.key.clone(), parent: e.parent, def });
        }

        let mut links = Vec::new();
        for (l, css) in self.links.iter().zip(link_css) {
            let label = l.tok.label.as_ref().and_then(|t| self.ctx.reg.add_opt(text::runs(t, &css.text(&edge_base)), None));
            let mut arrow = Arrow::plain(l.tok.len);
            arrow.tail = l.tok.tail;
            arrow.head = l.tok.head;
            arrow.dashed = l.tok.dashed || css.dash.is_some();
            arrow.hidden = l.tok.hidden;
            arrow.bold = l.tok.thick;
            arrow.width = Some(css.stroke_width.unwrap_or(if l.tok.thick { THICK_WIDTH } else { NORMAL_WIDTH }));
            arrow.color.clone_from(&css.stroke);
            links.push(Link { from: l.from, to: l.to, arrow, label, label_dir: None, from_card: None, to_card: None, line: l.line });
        }

        Ok(Model { ents, links, dir: header.dir.unwrap_or_default() })
    }
}

pub fn parse(lines: &[Line], header: &Header, ctx: &mut Ctx) -> Result<Model, String> {
    let mut p = Parser {
        ctx,
        ents: Vec::new(),
        index: HashMap::new(),
        links: Vec::new(),
        stack: Vec::new(),
        classes: Classes::default(),
        styles: Vec::new(),
        assigns: Vec::new(),
        link_default: Css::default(),
        link_styles: Vec::new(),
    };
    for line in &join_props(lines) {
        p.statement(line)?;
    }
    p.finish(header)
}

/// Joins the lines of an `@{ ... }` block that is still open at the end of its line.
fn join_props(lines: &[Line]) -> Vec<Line> {
    let mut out: Vec<Line> = Vec::new();
    let mut open = false;
    for l in lines {
        if open {
            let last = out.last_mut().unwrap();
            last.text.push('\n');
            last.text.push_str(&l.text);
        } else {
            out.push(l.clone());
        }
        let text = &out.last().unwrap().text;
        open = text.rfind("@{").is_some_and(|p| !text[p..].contains('}'));
    }
    out
}
