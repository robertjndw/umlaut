//! Mermaid sequence diagrams, parsed into PlantUML's sequence model and drawn by its
//! layout.
//!
//! Labels are registered in source order while parsing: message, note and block texts
//! as the statements are read, then the title, the participants (whose display name may
//! be changed by a later `participant X as ...`) at the end. Styling can never come
//! after the thing it styles in a sequence diagram, so there is no separate collecting
//! phase, and the order is still a pure function of the source.
//!
//! Mermaid's `rect` regions are the model's `Highlight` events; everything else maps to
//! events PlantUML can produce too.

use std::collections::HashMap;

use super::lexer::Header;
use super::{set_title, text, theme};
use crate::color::parse_css;
use crate::plantuml::diagrams::sequence::{
    Arrow, BoxDecl, Effect, EffectKind, Endpoint, Event, Message, Model as PModel, Note, NotePlace, PKind, Participant,
};
use crate::plantuml::layout::shapes::{Head, NoteKind};
use crate::plantuml::layout::Body;
use crate::plantuml::lexer::{check_nesting, err, first_word, rest_after_word, Line};
use crate::plantuml::model::{Ctx, LabelId, Measured};
use crate::plantuml::skin::Skin;
use crate::scene::{Paint, Run};

pub type Model = PModel;

pub fn layout(m: &Model, skin: &Skin, meas: &Measured) -> Result<Body, String> {
    Ok(crate::plantuml::layout::sequence::layout(m, skin, meas))
}

/// Mermaid wraps message text at the distance between two actors (150 wide, 50 margin)
/// and note text at the note width.
const MESSAGE_WRAP: f32 = 200.0;
const NOTE_WRAP: f32 = 150.0;

/// Arrow tokens, longest first so `-->>` is never read as `-->` plus `>`.
/// Token, dashed, head at the start, head at the end.
type ArrowTok = (&'static str, bool, Head, Head);

const ARROWS: &[ArrowTok] = &[
    ("<<-->>", true, Head::Point, Head::Point),
    ("<<->>", false, Head::Point, Head::Point),
    ("-->>", true, Head::None, Head::Point),
    ("->>", false, Head::None, Head::Point),
    ("-->", true, Head::None, Head::None),
    ("--x", true, Head::None, Head::Cross),
    ("--)", true, Head::None, Head::Open),
    ("->", false, Head::None, Head::None),
    ("-x", false, Head::None, Head::Cross),
    ("-)", false, Head::None, Head::Open),
];

#[derive(Clone, Copy, PartialEq)]
enum Block {
    Loop,
    Alt,
    Opt,
    Par,
    Critical,
    Break,
    Rect,
    Box,
}

impl Block {
    fn keyword(self) -> &'static str {
        match self {
            Block::Loop => "loop",
            Block::Alt => "alt",
            Block::Opt => "opt",
            Block::Par => "par",
            Block::Critical => "critical",
            Block::Break => "break",
            Block::Rect => "rect",
            Block::Box => "box",
        }
    }
}

struct Decl {
    id: String,
    kind: PKind,
    display: String,
    box_idx: Option<usize>,
}

struct Autonumber {
    next: f64,
    step: f64,
}

struct Parser<'a> {
    ctx: &'a mut Ctx,
    decls: Vec<Decl>,
    index: HashMap<String, usize>,
    boxes: Vec<BoxDecl>,
    open_box: Option<usize>,
    events: Vec<Event>,
    /// Open blocks with the line that opened them.
    stack: Vec<(Block, usize)>,
    auto: Option<Autonumber>,
    title: Option<String>,
    pending_create: Option<(usize, usize)>,
    pending_destroy: Vec<(usize, usize)>,
}

pub fn parse(lines: &[Line], header: &Header, ctx: &mut Ctx) -> Result<Model, String> {
    let mut p = Parser {
        ctx,
        decls: Vec::new(),
        index: HashMap::new(),
        boxes: Vec::new(),
        open_box: None,
        events: Vec::new(),
        stack: Vec::new(),
        auto: None,
        title: None,
        pending_create: None,
        pending_destroy: Vec::new(),
    };
    for l in lines {
        if !l.text.is_empty() {
            p.statement(l)?;
        }
    }
    if let Some(&(b, line)) = p.stack.last() {
        return err(line, format!("`{}` is never closed with `end`", b.keyword()));
    }
    if let Some((_, line)) = p.pending_create {
        return err(line, "`create` must be followed by a message to the created participant");
    }
    if let Some(&(_, line)) = p.pending_destroy.first() {
        return err(line, "`destroy` must be followed by a message involving the participant");
    }
    p.finish(header)
}

/// The first `:` splits a statement from its text; participant names cannot contain one.
fn split_colon(s: &str) -> (&str, Option<&str>) {
    match s.find(':') {
        Some(i) => (s[..i].trim(), Some(s[i + 1..].trim())),
        None => (s.trim(), None),
    }
}

/// Leftmost arrow in `s`, as (start, length, token). Several tokens can start at one
/// position, the table order picks the longest.
fn find_arrow(s: &str) -> Option<(usize, &'static ArrowTok)> {
    s.char_indices().find_map(|(i, _)| ARROWS.iter().find(|a| s[i..].starts_with(a.0)).map(|a| (i, a)))
}

fn pkind(word: &str) -> Option<PKind> {
    Some(match word {
        "participant" => PKind::Participant,
        "actor" => PKind::Actor,
        "boundary" => PKind::Boundary,
        "control" => PKind::Control,
        "entity" => PKind::Entity,
        "database" => PKind::Database,
        "collections" => PKind::Collections,
        "queue" => PKind::Queue,
        _ => return None,
    })
}

/// Value of `key` in the loose JSON of `@{ "type": "database", alias: "x" }`.
fn json_field(body: &str, key: &str) -> Option<String> {
    let mut from = 0;
    while let Some(i) = body[from..].find(key) {
        let at = from + i;
        from = at + key.len();
        let before = body[..at].trim_end_matches('"').trim_end();
        let after = body[at + key.len()..].trim_start_matches('"').trim_start();
        let boundary = before.is_empty() || before.ends_with(['{', ',']);
        let Some(value) = after.strip_prefix(':').filter(|_| boundary) else { continue };
        let value = value.trim_start();
        if let Some(q) = value.strip_prefix('"') {
            return q.find('"').map(|e| q[..e].to_string());
        }
        let end = value.find([',', '}']).unwrap_or(value.len());
        return Some(value[..end].trim().to_string());
    }
    None
}

impl Parser<'_> {
    fn ensure(&mut self, id: &str) -> usize {
        if let Some(&i) = self.index.get(id) {
            return i;
        }
        self.decls.push(Decl { id: id.to_string(), kind: PKind::Participant, display: id.to_string(), box_idx: self.open_box });
        self.index.insert(id.to_string(), self.decls.len() - 1);
        self.decls.len() - 1
    }

    fn wrap_default(&self) -> bool {
        self.ctx.skin.flag("wrap") == Some(true)
    }

    fn group_base(&self) -> crate::plantuml::creole::Base {
        self.ctx.skin.text(&["sequencegroup", "sequencegroupheader"], theme::FONT_SIZE, theme::ACTOR_TEXT)
    }

    /// Message text with its `wrap:` / `nowrap:` marker removed, and whether to wrap.
    fn wrap_marker<'t>(&self, t: &'t str) -> (&'t str, bool) {
        for (prefix, wrap) in [("wrap:", true), ("nowrap:", false)] {
            if t.get(..prefix.len()).is_some_and(|h| h.eq_ignore_ascii_case(prefix)) {
                return (t[prefix.len()..].trim_start(), wrap);
            }
        }
        (t, self.wrap_default())
    }

    fn number_runs(&mut self, base: &crate::plantuml::creole::Base) -> Vec<Run> {
        let Some(a) = self.auto.as_mut() else { return vec![] };
        let n = a.next;
        a.next += a.step;
        let shown = if n.fract() == 0.0 { format!("{}", n as i64) } else { format!("{n:.2}").trim_end_matches('0').to_string() };
        let mut runs = text::plain_runs(&shown, &base.clone().bold());
        runs.push(Run::new(" ").size(base.size));
        runs
    }

    fn statement(&mut self, l: &Line) -> Result<(), String> {
        let t = l.text.as_str();
        let lower = first_word(t).to_ascii_lowercase();
        let word = lower.trim_end_matches(':');
        let rest = rest_after_word(t);
        if let Some(kind) = pkind(word) {
            return self.declare(l, kind, rest, false);
        }
        match word {
            "create" => return self.create(l, rest),
            "activate" | "deactivate" | "destroy" => return self.life_event(l, word, rest),
            "autonumber" => return self.autonumber(l, rest),
            "title" => {
                self.title = Some(rest.trim_start_matches(':').trim().to_string());
                return Ok(());
            }
            "loop" => return self.open(l, Block::Loop, rest),
            "alt" => return self.open(l, Block::Alt, rest),
            "opt" => return self.open(l, Block::Opt, rest),
            "par" => return self.open(l, Block::Par, rest),
            "critical" => return self.open(l, Block::Critical, rest),
            "break" => return self.open(l, Block::Break, rest),
            "rect" => return self.rect(l, rest),
            "box" => return self.box_start(l, rest),
            "else" => return self.divide(l, Block::Alt, "else", rest),
            "and" => return self.divide(l, Block::Par, "and", rest),
            "option" => return self.divide(l, Block::Critical, "option", rest),
            "end" if rest.is_empty() => return self.end(l),
            "note" => return self.note(l, rest),
            "link" | "links" | "properties" | "details" => {
                self.ctx.warn(l.no, &format!("`{word}` is not supported and is ignored"));
                return Ok(());
            }
            _ => {}
        }
        self.message(l)
    }

    fn declare(&mut self, l: &Line, kind: PKind, rest: &str, create: bool) -> Result<(), String> {
        let rest = rest.trim();
        let id_end = rest.find(|c: char| c.is_whitespace() || c == '@').unwrap_or(rest.len());
        let id = rest[..id_end].trim_matches('"');
        if id.is_empty() {
            return err(l.no, format!("{} needs a name", kind.keyword()));
        }
        let mut after = rest[id_end..].trim_start();
        let (mut kind, mut display) = (kind, id.to_string());
        if let Some(cfg) = after.strip_prefix("@{") {
            let Some(end) = cfg.find('}') else { return err(l.no, "unterminated `@{` in the participant declaration") };
            let body = &cfg[..end];
            if let Some(ty) = json_field(body, "type") {
                kind = match pkind(&ty.to_ascii_lowercase()) {
                    Some(k) => k,
                    None => return err(l.no, format!("unknown participant type \"{ty}\"")),
                };
            }
            if let Some(alias) = json_field(body, "alias") {
                display = alias;
            }
            after = cfg[end + 1..].trim_start();
        }
        if after.get(..2).is_some_and(|w| w.eq_ignore_ascii_case("as")) && after[2..].starts_with(char::is_whitespace) {
            display = after[2..].trim().to_string();
        } else if !after.is_empty() {
            return err(l.no, format!("cannot understand \"{after}\" in the {} declaration", kind.keyword()));
        }
        let idx = self.ensure(id);
        self.decls[idx].kind = kind;
        self.decls[idx].display = display;
        if create {
            self.pending_create = Some((idx, l.no));
        }
        Ok(())
    }

    fn create(&mut self, l: &Line, rest: &str) -> Result<(), String> {
        match pkind(&first_word(rest).to_ascii_lowercase()) {
            Some(kind) => self.declare(l, kind, rest_after_word(rest), true),
            None => err(l.no, "`create` needs `participant` or `actor` and a name"),
        }
    }

    fn life_event(&mut self, l: &Line, word: &str, rest: &str) -> Result<(), String> {
        let name = rest.trim();
        if name.is_empty() {
            return err(l.no, format!("`{word}` needs a participant name"));
        }
        let who = self.ensure(name);
        if word == "destroy" {
            self.pending_destroy.push((who, l.no));
            return Ok(());
        }
        let kind = if word == "activate" { EffectKind::Activate } else { EffectKind::Deactivate };
        let effect = Effect { kind, who, color: None };
        // Attach to the message right before, so the arrow lands on the bar.
        if let Some(Event::Message(m)) = self.events.last_mut() {
            if m.from == Endpoint::Part(who) || m.to == Endpoint::Part(who) {
                m.effects.push(effect);
                return Ok(());
            }
        }
        self.events.push(Event::Effect(effect));
        Ok(())
    }

    fn autonumber(&mut self, l: &Line, rest: &str) -> Result<(), String> {
        let toks: Vec<&str> = rest.split_whitespace().collect();
        if toks.first().is_some_and(|t| t.eq_ignore_ascii_case("off")) {
            self.auto = None;
            return Ok(());
        }
        let mut nums = Vec::new();
        for t in &toks {
            match t.parse::<f64>() {
                Ok(n) if n.is_finite() => nums.push(n),
                _ => return err(l.no, format!("cannot understand \"{t}\" in autonumber")),
            }
        }
        if nums.len() > 2 {
            return err(l.no, "autonumber takes a start and a step at most");
        }
        self.auto = Some(Autonumber { next: nums.first().copied().unwrap_or(1.0), step: nums.get(1).copied().unwrap_or(1.0) });
        Ok(())
    }

    fn cond(&mut self, text: &str) -> Option<LabelId> {
        let t = text.trim();
        if t.is_empty() {
            return None;
        }
        let runs = text::runs(&format!("[{t}]"), &self.group_base());
        self.ctx.reg.add_opt(runs, None)
    }

    fn open(&mut self, l: &Line, block: Block, rest: &str) -> Result<(), String> {
        check_nesting(self.stack.len(), l.no)?;
        let tab = self.ctx.reg.add(text::runs(block.keyword(), &self.group_base()), None);
        let cond = self.cond(rest);
        self.events.push(Event::GroupStart { tab, cond });
        self.stack.push((block, l.no));
        Ok(())
    }

    fn divide(&mut self, l: &Line, inside: Block, word: &str, rest: &str) -> Result<(), String> {
        if self.stack.last().map(|b| b.0) != Some(inside) {
            return err(l.no, format!("`{word}` is only allowed inside `{}`", inside.keyword()));
        }
        let cond = self.cond(rest);
        self.events.push(Event::GroupElse(cond));
        Ok(())
    }

    fn rect(&mut self, l: &Line, rest: &str) -> Result<(), String> {
        let Some(color) = parse_css(rest.trim()) else {
            return err(l.no, format!("`rect` needs a color such as rgb(0, 255, 0), found \"{rest}\""));
        };
        check_nesting(self.stack.len(), l.no)?;
        self.events.push(Event::Highlight(Paint::solid(&color)));
        self.stack.push((Block::Rect, l.no));
        Ok(())
    }

    fn box_start(&mut self, l: &Line, rest: &str) -> Result<(), String> {
        if self.open_box.is_some() || !self.stack.is_empty() {
            return err(l.no, "boxes cannot be nested or placed inside a block");
        }
        // The color comes first and may be a function call with spaces in it.
        let first_end = match rest.find('(') {
            Some(p) if !rest[..p].contains(char::is_whitespace) => rest.find(')').map_or(rest.len(), |e| e + 1),
            _ => rest.find(char::is_whitespace).unwrap_or(rest.len()),
        };
        let (mut fill, mut name) = (None, rest);
        if let Some(c) = parse_css(&rest[..first_end]) {
            fill = Some(Paint::solid(&c));
            name = rest[first_end..].trim();
        }
        let label = if name.is_empty() { None } else { self.ctx.reg.add_opt(text::runs(name, &self.group_base()), None) };
        self.boxes.push(BoxDecl { label, fill });
        self.open_box = Some(self.boxes.len() - 1);
        self.stack.push((Block::Box, l.no));
        Ok(())
    }

    fn end(&mut self, l: &Line) -> Result<(), String> {
        match self.stack.pop() {
            None => return err(l.no, "`end` without a block to close"),
            Some((Block::Box, _)) => self.open_box = None,
            Some((Block::Rect, _)) => self.events.push(Event::HighlightEnd),
            Some(_) => self.events.push(Event::GroupEnd),
        }
        Ok(())
    }

    fn note(&mut self, l: &Line, rest: &str) -> Result<(), String> {
        let (head, body) = split_colon(rest);
        let word = first_word(head).to_ascii_lowercase();
        let mut who = rest_after_word(head);
        let place = match word.as_str() {
            "left" | "right" => {
                if first_word(who).eq_ignore_ascii_case("of") {
                    who = rest_after_word(who);
                }
                if who.is_empty() {
                    return err(l.no, "the note needs a participant after `of`");
                }
                let p = self.ensure(who);
                if word == "left" { NotePlace::Left(p) } else { NotePlace::Right(p) }
            }
            "over" => {
                let names: Vec<&str> = who.split(',').map(str::trim).collect();
                if names.iter().any(|n| n.is_empty()) {
                    return err(l.no, "the note needs a participant after `over`");
                }
                NotePlace::Over(names.into_iter().map(|n| self.ensure(n)).collect())
            }
            _ => return err(l.no, format!("cannot understand the note \"note {rest}\"")),
        };
        let Some(body) = body else { return err(l.no, "the note needs text after a `:`") };
        let (body, wrap) = self.wrap_marker(body);
        let runs = text::runs(body, &theme::note(&self.ctx.skin));
        let Some(label) = self.ctx.reg.add_opt(runs, wrap.then_some(NOTE_WRAP)) else { return err(l.no, "the note is empty") };
        self.events.push(Event::Note(Note { kind: NoteKind::Rect, place, fill: None, label }));
        Ok(())
    }

    fn message(&mut self, l: &Line) -> Result<(), String> {
        let (head, label_text) = split_colon(&l.text);
        let Some((at, &(tok, dashed, head_start, head_end))) = find_arrow(head) else {
            return err(l.no, format!("syntax error, cannot understand \"{}\"", l.text));
        };
        let from_name = head[..at].trim();
        let mut to_name = head[at + tok.len()..].trim();
        let (mut activate, mut deactivate) = (false, false);
        if let Some(r) = to_name.strip_prefix('+') {
            (activate, to_name) = (true, r.trim_start());
        } else if let Some(r) = to_name.strip_prefix('-') {
            (deactivate, to_name) = (true, r.trim_start());
        }
        if from_name.is_empty() || to_name.is_empty() {
            return err(l.no, format!("the arrow needs a participant on both sides in \"{}\"", l.text));
        }
        let (from, to) = (self.ensure(from_name), self.ensure(to_name));

        let mut effects = Vec::new();
        if activate {
            effects.push(Effect { kind: EffectKind::Activate, who: to, color: None });
        }
        if deactivate {
            // `-` ends the activation of the sender: `B-->>-A` returns from B.
            effects.push(Effect { kind: EffectKind::Deactivate, who: from, color: None });
        }
        let mut i = 0;
        while i < self.pending_destroy.len() {
            let who = self.pending_destroy[i].0;
            if who == from || who == to {
                effects.push(Effect { kind: EffectKind::Destroy, who, color: None });
                self.pending_destroy.remove(i);
            } else {
                i += 1;
            }
        }
        let mut creates = false;
        if let Some((p, line)) = self.pending_create {
            if to != p {
                return err(line, "`create` must be followed by a message to the created participant");
            }
            creates = true;
            self.pending_create = None;
        }

        let (body, wrap) = self.wrap_marker(label_text.unwrap_or(""));
        let base = theme::message(&self.ctx.skin);
        let mut runs = self.number_runs(&base);
        runs.extend(text::runs(body, &base));
        let label = self.ctx.reg.add_opt(runs, wrap.then_some(MESSAGE_WRAP));
        self.events.push(Event::Message(Message {
            from: Endpoint::Part(from),
            to: Endpoint::Part(to),
            arrow: Arrow { dashed, color: None, thickness: None, hidden: false, head_start, head_end },
            label,
            effects,
            creates,
            line: l.no,
        }));
        Ok(())
    }

    fn finish(mut self, header: &Header) -> Result<Model, String> {
        if self.decls.is_empty() {
            return err(header.line, "Mermaid sequence diagrams need at least one participant");
        }
        if let Some(t) = self.title.take().filter(|t| !t.is_empty()) {
            set_title(self.ctx, &t);
        }
        let mut participants = Vec::new();
        for d in &self.decls {
            let base = theme::actor(&self.ctx.skin, d.kind.keyword());
            let label = self.ctx.reg.add(text::runs(&d.display, &base), None);
            participants.push(Participant { id: d.id.clone(), kind: d.kind, label, fill: None, box_idx: d.box_idx });
        }
        Ok(Model { participants, boxes: self.boxes, events: self.events, hide_footbox: false })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mermaid::{self, lexer};

    fn model(src: &str) -> Result<(Model, Ctx), String> {
        let lexer::Source { header, lines, wrap, .. } = lexer::lex(src.as_bytes())?;
        let mut ctx = Ctx { skin: theme::skin(), reg: Default::default(), meta: Default::default(), warnings: Vec::new() };
        if wrap {
            ctx.skin.set("wrap", "true");
        }
        let m = parse(&lines, &header, &mut ctx)?;
        Ok((m, ctx))
    }

    fn msgs(m: &Model) -> Vec<&Message> {
        m.events.iter().filter_map(|e| if let Event::Message(m) = e { Some(m) } else { None }).collect()
    }

    fn fail(src: &str) -> String {
        model(src).expect_err("should fail")
    }

    #[test]
    fn arrows_map_to_heads_and_dashes() {
        let src = "sequenceDiagram\nA->B\nA-->B\nA->>B\nA-->>B\nA<<->>B\nA<<-->>B\nA-xB\nA--xB\nA-)B\nA--)B";
        let (m, _) = model(src).unwrap();
        let got: Vec<_> = msgs(&m).iter().map(|m| (m.arrow.dashed, m.arrow.head_start, m.arrow.head_end)).collect();
        let want = [
            (false, Head::None, Head::None),
            (true, Head::None, Head::None),
            (false, Head::None, Head::Point),
            (true, Head::None, Head::Point),
            (false, Head::Point, Head::Point),
            (true, Head::Point, Head::Point),
            (false, Head::None, Head::Cross),
            (true, Head::None, Head::Cross),
            (false, Head::None, Head::Open),
            (true, Head::None, Head::Open),
        ];
        assert_eq!(got, want);
    }

    #[test]
    fn participants_are_ordered_by_appearance_and_aliased() {
        let (m, c) = model("sequenceDiagram\nactor B as Bob\nparticipant A as Alice\nA->>C: x\nB->>A: y\nself->>self: z").unwrap();
        let ids: Vec<_> = m.participants.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["B", "A", "C", "self"]);
        assert_eq!(m.participants[0].kind, PKind::Actor);
        let name = |i: usize| c.reg.labels[m.participants[i].label.0].runs[0].text.clone();
        assert_eq!((name(0).as_str(), name(1).as_str()), ("Bob", "Alice"));
    }

    #[test]
    fn json_types_and_names_with_spaces() {
        let (m, _) = model("sequenceDiagram\nparticipant D@{ \"type\": \"database\" } as Store\nparticipant Q@{ \"type\" : \"queue\" }\nactor E@{ \"type\": \"entity\" }\nAlice Smith->>D: hi").unwrap();
        let kinds: Vec<_> = m.participants.iter().map(|p| p.kind).collect();
        assert_eq!(kinds, [PKind::Database, PKind::Queue, PKind::Entity, PKind::Participant]);
        assert_eq!(m.participants[3].id, "Alice Smith");
        assert!(fail("sequenceDiagram\nparticipant D@{ \"type\": \"blob\" }").contains("unknown participant type"));
    }

    #[test]
    fn activation_shorthand_targets_the_right_side() {
        let (m, _) = model("sequenceDiagram\nA->>+B: call\nB-->>-A: back").unwrap();
        let ms = msgs(&m);
        assert_eq!((ms[0].effects[0].kind, ms[0].effects[0].who), (EffectKind::Activate, 1));
        assert_eq!((ms[1].effects[0].kind, ms[1].effects[0].who), (EffectKind::Deactivate, 1));
        assert_eq!(ms[1].to, Endpoint::Part(0));
    }

    #[test]
    fn activate_statement_folds_into_the_previous_message() {
        let (m, _) = model("sequenceDiagram\nactivate C\nA->>B: x\nactivate B\nA->>B: y\ndeactivate A").unwrap();
        assert!(matches!(m.events[0], Event::Effect(_)));
        assert_eq!(msgs(&m)[0].effects.len(), 1);
        assert_eq!(msgs(&m)[1].effects[0].kind, EffectKind::Deactivate);
    }

    #[test]
    fn blocks_nest_and_close() {
        let src = "sequenceDiagram\nloop Every min\nalt ok\nA->>B: x\nelse bad\nA->>B: y\nend\nend\npar a\nA->>B: x\nand b\nA->>B: y\nend\ncritical c\nA->>B: z\noption o\nA->>B: w\nend\nopt q\nbreak b\nend\nend";
        let (m, c) = model(src).unwrap();
        let starts = m.events.iter().filter(|e| matches!(e, Event::GroupStart { .. })).count();
        let ends = m.events.iter().filter(|e| matches!(e, Event::GroupEnd)).count();
        assert_eq!((starts, ends), (6, 6));
        let Event::GroupStart { tab, cond } = &m.events[0] else { panic!() };
        assert_eq!(c.reg.labels[tab.0].runs[0].text, "loop");
        assert_eq!(c.reg.labels[cond.unwrap().0].runs[0].text, "[Every min]");
    }

    #[test]
    fn block_errors_carry_line_numbers() {
        assert_eq!(fail("sequenceDiagram\nA->>B: x\nloop forever\nA->>B: y"), "line 3: `loop` is never closed with `end`");
        assert_eq!(fail("sequenceDiagram\nA->>B: x\nend"), "line 3: `end` without a block to close");
        assert!(fail("sequenceDiagram\nloop x\nelse y\nend").starts_with("line 3: `else` is only allowed inside `alt`"));
        assert!(fail("sequenceDiagram\nA->>B: x\nbogus words here").starts_with("line 3: syntax error"));
        assert!(fail("sequenceDiagram\nrect nonsense\nend").starts_with("line 2:"));
    }

    #[test]
    fn rect_makes_highlight_events_that_nest_with_groups() {
        let (m, _) = model("sequenceDiagram\nrect rgba(0, 0, 255, .1)\nloop x\nA->>B: m\nend\nend").unwrap();
        assert!(matches!(&m.events[0], Event::Highlight(p) if p.color == "#0000ff1a"));
        assert!(matches!(m.events.last(), Some(Event::HighlightEnd)));
        assert!(matches!(m.events[1], Event::GroupStart { .. }));
    }

    #[test]
    fn notes_are_case_insensitive() {
        let (m, _) = model("sequenceDiagram\nNOTE right of A: a\nnote left of B: b\nNote over A,B: c\nnote over C: d").unwrap();
        let places: Vec<_> = m.events.iter().map(|e| match e { Event::Note(n) => n.place.clone(), _ => unreachable!() }).collect();
        assert!(matches!(places[0], NotePlace::Right(0)));
        assert!(matches!(places[1], NotePlace::Left(1)));
        assert!(matches!(&places[2], NotePlace::Over(v) if v == &[0, 1]));
        assert!(matches!(&places[3], NotePlace::Over(v) if v == &[2]));
        assert!(matches!(&m.events[0], Event::Note(n) if n.kind == NoteKind::Rect));
    }

    #[test]
    fn create_and_destroy_attach_to_the_next_message() {
        let (m, _) = model("sequenceDiagram\nA->>B: x\ncreate participant C\nA->>C: hi\ndestroy C\nA-xC: bye").unwrap();
        let ms = msgs(&m);
        assert!(ms[1].creates && !ms[0].creates);
        assert_eq!(ms[2].effects[0].kind, EffectKind::Destroy);
        assert!(fail("sequenceDiagram\ncreate participant C\nA->>B: hi").contains("line 2"));
        assert!(fail("sequenceDiagram\nA->>B: hi\ndestroy B").starts_with("line 3"));
    }

    #[test]
    fn boxes_group_participants_and_take_a_color() {
        let (m, c) = model("sequenceDiagram\nbox Aqua Front end\nparticipant A\nparticipant B\nend\nbox rgb(1, 2, 3) Back\nparticipant C\nend\nbox plain\nparticipant D\nend\nbox transparent\nparticipant E\nend\nF->>A: x").unwrap();
        assert_eq!(m.boxes.len(), 4);
        assert_eq!(m.participants.iter().map(|p| p.box_idx).collect::<Vec<_>>(), [Some(0), Some(0), Some(1), Some(2), Some(3), None]);
        assert_eq!(m.boxes[0].fill.as_ref().unwrap().color, "#00ffffff");
        assert_eq!(c.reg.labels[m.boxes[0].label.unwrap().0].runs[0].text, "Front end");
        assert_eq!(m.boxes[1].fill.as_ref().unwrap().color, "#010203ff");
        assert!(m.boxes[2].fill.is_none());
        assert_eq!(c.reg.labels[m.boxes[2].label.unwrap().0].runs[0].text, "plain");
        assert!(m.boxes[3].label.is_none());
    }

    #[test]
    fn autonumber_prefixes_messages_and_can_be_stopped() {
        let (m, c) = model("sequenceDiagram\nA->>B: before\nautonumber 10 5\nA->>B: x\nB->>A\nautonumber off\nA->>B: after").unwrap();
        let text = |i: usize| c.reg.labels[msgs(&m)[i].label.unwrap().0].runs.iter().map(|r| r.text.clone()).collect::<String>();
        assert_eq!(text(0), "before");
        assert_eq!(text(1), "10 x");
        assert_eq!(text(2).trim(), "15");
        assert_eq!(text(3), "after");
    }

    #[test]
    fn wrap_comes_from_the_flag_or_the_prefix() {
        let (m, c) = model("sequenceDiagram\nA->>B: wrap: long text\nA->>B: short").unwrap();
        let wrap = |i: usize| c.reg.labels[msgs(&m)[i].label.unwrap().0].wrap_width;
        assert_eq!((wrap(0), wrap(1)), (Some(MESSAGE_WRAP), None));
        let (m, c) = model("%%{init: {\"sequence\": {\"wrap\": true}}}%%\nsequenceDiagram\nA->>B: hi\nA->>B: nowrap: hi").unwrap();
        let wrap = |i: usize| c.reg.labels[msgs(&m)[i].label.unwrap().0].wrap_width;
        assert_eq!((wrap(0), wrap(1)), (Some(MESSAGE_WRAP), None));
    }

    #[test]
    fn links_are_ignored_with_a_warning_and_title_is_registered() {
        let (m, c) = model("sequenceDiagram\ntitle Hello\nparticipant A\nlink A: Docs @ https://x.org\nA->>A: hi").unwrap();
        assert_eq!(c.warnings.len(), 1);
        assert!(c.warnings[0].starts_with("line 4: "));
        assert_eq!(msgs(&m).len(), 1);
        assert!(c.meta.title.is_some());
    }

    #[test]
    fn entities_and_breaks_reach_the_text() {
        let (m, c) = model("sequenceDiagram\nA->>B: one<br/>two #9829; ok").unwrap();
        let runs = &c.reg.labels[msgs(&m)[0].label.unwrap().0].runs;
        assert!(runs.iter().any(|r| r.br));
        assert!(runs.iter().any(|r| r.text.contains('\u{2665}')));
    }

    #[test]
    fn fixtures_render_deterministically_with_valid_geometry() {
        for (name, src) in crate::test_util::fixtures("mermaid", "mmd", "seq_") {
            let render = || crate::test_util::render_with(mermaid::labels, mermaid::layout, &src).unwrap_or_else(|e| panic!("{name}: {e}"));
            let (a, b) = (render(), render());
            assert_eq!(format!("{a:?}"), format!("{b:?}"), "{name} is not deterministic");
            assert!(a.size[0] > 0.0 && a.size[1] > 0.0, "{name}");
        }
    }

    #[test]
    fn truncated_sources_never_panic() {
        let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/mermaid/seq_blocks.mmd")).unwrap();
        for end in (0..src.len()).filter(|&i| src.is_char_boundary(i)) {
            let _ = mermaid::labels(&src.as_bytes()[..end]);
        }
    }
}
