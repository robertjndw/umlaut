//! Sequence diagrams: statements to a flat list of events.
//!
//! Participants are auto-created on first use. Activation shorthands and `activate`
//! lines that directly follow a message are folded into that message (`Message::effects`)
//! so the layout knows, before it draws the arrow, where the arrow has to end.

use std::collections::HashMap;

use crate::plantuml::colors::{parse_color, parse_paint};
use crate::plantuml::creole;
use crate::plantuml::lexer::{err, first_word, is_end, rest_after_word, unquote, Cursor, Line};
use crate::plantuml::model::{Ctx, LabelId};
use crate::plantuml::layout::shapes::{Head, NoteKind};
use crate::scene::{Paint, Run};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PKind {
    Participant,
    Actor,
    Boundary,
    Control,
    Entity,
    Database,
    Collections,
    Queue,
}

impl PKind {
    pub fn keyword(self) -> &'static str {
        match self {
            PKind::Participant => "participant",
            PKind::Actor => "actor",
            PKind::Boundary => "boundary",
            PKind::Control => "control",
            PKind::Entity => "entity",
            PKind::Database => "database",
            PKind::Collections => "collections",
            PKind::Queue => "queue",
        }
    }

    fn from_word(w: &str) -> Option<PKind> {
        Some(match w {
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
}

#[derive(Debug, Clone)]
pub struct Participant {
    pub id: String,
    pub kind: PKind,
    pub label: LabelId,
    pub fill: Option<Paint>,
    pub box_idx: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct BoxDecl {
    pub label: Option<LabelId>,
    pub fill: Option<Paint>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Endpoint {
    Part(usize),
    /// `[->`: comes from beyond the left edge of the diagram.
    Left,
    /// `->]`: goes beyond the right edge.
    Right,
}

#[derive(Debug, Clone)]
pub struct Arrow {
    pub dashed: bool,
    pub color: Option<String>,
    pub thickness: Option<f32>,
    pub hidden: bool,
    pub head_start: Head,
    pub head_end: Head,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EffectKind {
    Activate,
    Deactivate,
    Destroy,
}

#[derive(Debug, Clone)]
pub struct Effect {
    pub kind: EffectKind,
    pub who: usize,
    pub color: Option<Paint>,
}

#[derive(Debug, Clone)]
pub struct Message {
    pub from: Endpoint,
    pub to: Endpoint,
    pub arrow: Arrow,
    pub label: Option<LabelId>,
    pub effects: Vec<Effect>,
    /// The target is created by this message (`create X` or `->**`).
    pub creates: bool,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub enum NotePlace {
    Left(usize),
    Right(usize),
    Over(Vec<usize>),
    Across,
    /// `note left` / `note right` straight after a message.
    OnMessage { right: bool },
}

#[derive(Debug, Clone)]
pub struct Note {
    pub kind: NoteKind,
    pub place: NotePlace,
    pub fill: Option<Paint>,
    pub label: LabelId,
}

#[derive(Debug, Clone)]
pub enum Event {
    Message(Message),
    Note(Note),
    Ref { over: Vec<usize>, label: LabelId, tab: LabelId },
    Divider(LabelId),
    Delay(Option<LabelId>),
    Space(f32),
    GroupStart { tab: LabelId, cond: Option<LabelId> },
    GroupElse(Option<LabelId>),
    GroupEnd,
    /// Activation changes that do not directly follow a message.
    Effect(Effect),
}

#[derive(Debug)]
pub struct Model {
    pub participants: Vec<Participant>,
    pub boxes: Vec<BoxDecl>,
    pub events: Vec<Event>,
    pub hide_footbox: bool,
}

struct Decl {
    id: String,
    kind: PKind,
    display: String,
    stereotype: Option<String>,
    fill: Option<Paint>,
    order: Option<i64>,
    box_idx: Option<usize>,
}

struct Autonumber {
    next: i64,
    step: i64,
    format: Option<String>,
    active: bool,
}

struct Parser<'a> {
    ctx: &'a mut Ctx,
    decls: Vec<Decl>,
    index: HashMap<String, usize>,
    boxes: Vec<BoxDecl>,
    open_box: Option<usize>,
    events: Vec<Event>,
    hide_footbox: bool,
    auto: Option<Autonumber>,
    /// (caller, callee) of the activations still open, for `return`.
    returns: Vec<(Endpoint, usize)>,
    pending_create: Option<(usize, usize)>,
    groups: Vec<usize>,
    last_message: Option<(Endpoint, Endpoint)>,
}

pub fn parse(lines: &[Line], ctx: &mut Ctx) -> Result<Model, String> {
    let mut p = Parser {
        ctx,
        decls: Vec::new(),
        index: HashMap::new(),
        boxes: Vec::new(),
        open_box: None,
        events: Vec::new(),
        hide_footbox: false,
        auto: None,
        returns: Vec::new(),
        pending_create: None,
        groups: Vec::new(),
        last_message: None,
    };
    let mut cur = Cursor::new(lines);
    while let Some(l) = cur.next() {
        if l.text.is_empty() {
            continue;
        }
        p.statement(l, &mut cur)?;
    }
    if let Some(&start) = p.groups.last() {
        return err(start, "this group is never closed with `end`");
    }
    if let Some((_, line)) = p.pending_create {
        return err(line, "`create` must be followed by a message to the created participant");
    }
    p.finish()
}

/// Reads a participant name (quoted or bare) from the start of `s`.
fn read_name(s: &str) -> Option<(String, &str)> {
    let s = s.trim_start();
    if let Some(rest) = s.strip_prefix('"') {
        let end = rest.find('"')?;
        return Some((rest[..end].to_string(), &rest[end + 1..]));
    }
    let end = s
        .char_indices()
        .find(|&(i, c)| c.is_whitespace() || c == ':' || c == ',' || ["++", "--", "**", "!!"].iter().any(|op| s[i..].starts_with(op)))
        .map_or(s.len(), |(i, _)| i);
    if end == 0 {
        return None;
    }
    Some((s[..end].to_string(), &s[end..]))
}

#[derive(Debug, PartialEq)]
struct ArrowTok {
    start: usize,
    end: usize,
    left: Head,
    right: Head,
    dashes: usize,
    attrs: Option<String>,
}

fn head_from(s: &str) -> Head {
    match s {
        ">" | "<" => Head::Filled,
        ">>" | "<<" => Head::Open,
        "\\" => Head::HalfLower,
        "/" => Head::HalfUpper,
        "\\\\" | "//" => Head::Open,
        _ => Head::None,
    }
}

/// Finds the arrow in a message line: shaft of dashes (optionally with `[attrs]`),
/// with heads on either side. Skips quoted names.
fn find_arrow(s: &str) -> Option<ArrowTok> {
    let ch: Vec<(usize, char)> = s.char_indices().collect();
    let n = ch.len();
    let at = |i: usize| ch.get(i).map(|c| c.1);
    let byte = |i: usize| ch.get(i).map_or(s.len(), |c| c.0);
    let mut quoted = false;
    let mut i = 0;
    while i < n {
        let c = ch[i].1;
        if c == '"' {
            quoted = !quoted;
        }
        if quoted || c != '-' {
            i += 1;
            continue;
        }
        // Shaft.
        let mut j = i;
        let mut dashes = 0;
        let mut attrs = None;
        while at(j) == Some('-') {
            dashes += 1;
            j += 1;
        }
        if at(j) == Some('[') {
            if let Some(close) = (j..n).find(|&k| ch[k].1 == ']') {
                attrs = Some(s[byte(j + 1)..byte(close)].to_string());
                j = close + 1;
                while at(j) == Some('-') {
                    dashes += 1;
                    j += 1;
                }
            }
        }
        // Right head.
        let mut right = Head::None;
        let head_start = j;
        let mut k = j;
        match at(k) {
            Some('>') => {
                k += if at(k + 1) == Some('>') { 2 } else { 1 };
            }
            Some('\\') => k += if at(k + 1) == Some('\\') { 2 } else { 1 },
            Some('/') => k += if at(k + 1) == Some('/') { 2 } else { 1 },
            _ => {}
        }
        if k > head_start {
            right = head_from(&s[byte(head_start)..byte(k)]);
        }
        if matches!(at(k), Some('x') | Some('o')) && at(k + 1).is_none_or(|c| c.is_whitespace() || c == ']' || c == ':') {
            right = if at(k) == Some('x') { Head::Cross } else { Head::Circle };
            k += 1;
        }
        // Left head, directly before the shaft.
        let mut left = Head::None;
        let mut m = i;
        let l_end = i;
        match i.checked_sub(1).and_then(at) {
            Some('<') => m = i - if i >= 2 && at(i - 2) == Some('<') { 2 } else { 1 },
            Some('\\') => m = i - if i >= 2 && at(i - 2) == Some('\\') { 2 } else { 1 },
            Some('/') => m = i - if i >= 2 && at(i - 2) == Some('/') { 2 } else { 1 },
            _ => {}
        }
        if m < l_end {
            left = head_from(&s[byte(m)..byte(l_end)]);
        }
        let xo_at = m.checked_sub(1);
        if let Some(p) = xo_at {
            if matches!(at(p), Some('x') | Some('o')) && (p == 0 || at(p - 1).is_some_and(|c| c.is_whitespace() || c == '[')) {
                left = if at(p) == Some('x') { Head::Cross } else { Head::Circle };
                m = p;
            }
        }
        if right == Head::None && left == Head::None {
            i = j.max(i + 1);
            continue;
        }
        return Some(ArrowTok { start: byte(m), end: byte(k), left, right, dashes, attrs });
    }
    None
}

fn parse_arrow_attrs(attrs: Option<&str>, dashes: usize) -> (bool, Option<String>, Option<f32>, bool) {
    let mut dashed = dashes >= 2;
    let (mut color, mut thickness, mut hidden) = (None, None, false);
    for part in attrs.unwrap_or("").split(',') {
        let p = part.trim();
        let pl = p.to_ascii_lowercase();
        match pl.as_str() {
            "dashed" | "dotted" => dashed = true,
            "bold" => thickness = Some(2.0),
            "hidden" => hidden = true,
            "plain" => dashed = false,
            _ => {
                if let Some(v) = pl.strip_prefix("thickness=") {
                    thickness = v.parse().ok();
                } else if let Some(c) = parse_color(p) {
                    color = Some(c);
                }
            }
        }
    }
    (dashed, color, thickness, hidden)
}

impl Parser<'_> {
    fn ensure(&mut self, id: &str) -> usize {
        if let Some(&i) = self.index.get(id) {
            return i;
        }
        self.decls.push(Decl {
            id: id.to_string(),
            kind: PKind::Participant,
            display: id.to_string(),
            stereotype: None,
            fill: None,
            order: None,
            box_idx: self.open_box,
        });
        self.index.insert(id.to_string(), self.decls.len() - 1);
        self.decls.len() - 1
    }

    fn endpoint(&mut self, tok: &str, no: usize, left_side: bool) -> Result<Endpoint, String> {
        let t = tok.trim();
        if t == "[" || (left_side && t == "?") {
            return Ok(Endpoint::Left);
        }
        if t == "]" || (!left_side && t == "?") {
            return Ok(Endpoint::Right);
        }
        let (name, rest) = match read_name(t) {
            Some(x) => x,
            None => return err(no, format!("expected a participant name, found \"{t}\"")),
        };
        if !rest.trim().is_empty() {
            return err(no, format!("cannot understand \"{t}\" here, quote participant names that contain spaces"));
        }
        Ok(Endpoint::Part(self.ensure(&name)))
    }

    fn base(&self, prefixes: &[&str], size: f32) -> creole::Base {
        self.ctx.skin.text(prefixes, size, "#000000")
    }

    fn message_runs(&self, text: &str) -> Vec<Run> {
        creole::runs(text, &self.base(&["sequencemessage", "sequencearrow", "arrow"], 13.0))
    }

    fn wrap_width(&self) -> Option<f32> {
        let w = self.ctx.skin.num(&["maxmessagesize"], 0.0);
        (w > 0.0).then_some(w)
    }

    fn number_runs(&mut self) -> Vec<Run> {
        let base = self.base(&["sequencemessage", "sequencearrow", "arrow"], 13.0);
        let Some(a) = self.auto.as_mut().filter(|a| a.active) else { return vec![] };
        let n = a.next;
        a.next += a.step;
        let mut runs = match &a.format {
            None => creole::runs(&n.to_string(), &base.clone().bold()),
            Some(f) => {
                let zeros = f.chars().skip_while(|&c| c != '0').take_while(|&c| c == '0').count();
                let text = match f.find('0') {
                    Some(pos) if zeros > 0 => format!("{}{:0width$}{}", &f[..pos], n, &f[pos + zeros..], width = zeros),
                    _ => format!("{f}{n}"),
                };
                creole::runs(&text, &base)
            }
        };
        runs.push(Run::new(" ").size(base.size));
        runs
    }

    fn statement(&mut self, l: &Line, cur: &mut Cursor) -> Result<(), String> {
        let text = l.text.as_str();
        let lower = text.to_ascii_lowercase();
        let word = first_word(&lower);
        let rest = rest_after_word(text);

        if let Some(kind) = PKind::from_word(word) {
            return self.declare(l, kind, rest, false);
        }
        match word {
            "create" => return self.create(l, rest),
            "activate" | "deactivate" | "destroy" => return self.life_event(l, word, rest),
            "return" => return self.ret(l, rest),
            "autonumber" => return self.autonumber(l, rest),
            "alt" | "opt" | "loop" | "par" | "break" | "critical" | "group" => return self.group_start(l, word, rest),
            "else" | "also" => {
                let cond = (!rest.is_empty()).then(|| self.group_cond(rest));
                if self.groups.is_empty() {
                    return err(l.no, format!("`{word}` outside of a group"));
                }
                self.events.push(Event::GroupElse(cond));
                return Ok(());
            }
            "end" => return self.end(l, rest),
            "note" | "hnote" | "rnote" => return self.note(l, text, cur),
            "ref" if lower.starts_with("ref over") || lower.starts_with("ref  over") => return self.reference(l, cur),
            "box" => return self.box_start(l, rest),
            "hide" | "show" => {
                if rest.eq_ignore_ascii_case("footbox") {
                    self.hide_footbox = word == "hide";
                }
                return Ok(());
            }
            "newpage" => return err(l.no, "newpage is not supported"),
            "autoactivate" => {
                self.ctx.warn(l.no, "autoactivate is not supported and is ignored");
                return Ok(());
            }
            "skin" | "mainframe" => return Ok(()),
            _ => {}
        }
        if text == "..." || (text.starts_with("...") && text.ends_with("...") && text.len() > 6) {
            let inner = text.trim_matches('.').trim();
            let label = if inner.is_empty() { None } else { self.register_note_like(inner, &["sequencedelay"], 13.0, false) };
            self.events.push(Event::Delay(label));
            return Ok(());
        }
        if text.starts_with("==") && text.ends_with("==") && text.len() >= 4 {
            let inner = text[2..text.len() - 2].trim();
            let base = self.base(&["sequencedivider"], 13.0);
            let id = self.ctx.reg.add(creole::runs(inner, &base), None);
            self.events.push(Event::Divider(id));
            return Ok(());
        }
        if text.len() >= 4 && text.starts_with("||") && text.ends_with("||") {
            let inner = &text[2..text.len() - 2];
            if inner.is_empty() {
                self.events.push(Event::Space(20.0));
                return Ok(());
            }
            if let Ok(n) = inner.trim().parse::<f32>() {
                self.events.push(Event::Space(n));
                return Ok(());
            }
        }
        if text == "|||" {
            self.events.push(Event::Space(20.0));
            return Ok(());
        }
        self.message(l)
    }

    fn register_note_like(&mut self, text: &str, prefixes: &[&str], size: f32, bold: bool) -> Option<LabelId> {
        let mut base = self.base(prefixes, size);
        base.bold |= bold;
        let runs = creole::runs(text, &base);
        self.ctx.reg.add_opt(runs, None)
    }

    fn declare(&mut self, l: &Line, kind: PKind, rest: &str, create: bool) -> Result<(), String> {
        let mut rest = rest.trim();
        let Some((first, after)) = read_name(rest) else { return err(l.no, format!("{} needs a name", kind.keyword())) };
        let first_quoted = rest.starts_with('"');
        rest = after.trim_start();
        let (mut id, mut display) = (first.clone(), first.clone());
        // `get` because the byte after the keyword may sit inside a multi-byte character.
        if rest.get(..2).is_some_and(|w| w.eq_ignore_ascii_case("as")) && rest[2..].starts_with(char::is_whitespace) {
            rest = rest[2..].trim_start();
            let second_quoted = rest.starts_with('"');
            let Some((second, after2)) = read_name(rest) else { return err(l.no, "`as` needs a name") };
            rest = after2.trim_start();
            if first_quoted || !second_quoted {
                id = second;
            } else {
                display = second;
            }
        }
        let (mut stereotype, mut fill, mut order) = (None, None, None);
        while !rest.is_empty() {
            if let Some(r) = rest.strip_prefix("<<") {
                let Some(end) = r.find(">>") else { return err(l.no, "unterminated stereotype") };
                let mut st = r[..end].trim();
                // `<< (C,#ADD1B2) Name >>`: the colored spot is not drawn.
                if st.starts_with('(') {
                    st = st.split_once(')').map_or(st, |(_, r)| r).trim();
                }
                stereotype = Some(st.to_string());
                rest = r[end + 2..].trim_start();
            } else if rest.get(..5).is_some_and(|w| w.eq_ignore_ascii_case("order")) {
                let r = rest[5..].trim_start();
                let num: String = r.chars().take_while(|c| c.is_ascii_digit() || *c == '-').collect();
                order = Some(num.parse::<i64>().map_err(|_| format!("line {}: `order` needs a number", l.no))?);
                rest = r[num.len()..].trim_start();
            } else if rest.starts_with('#') {
                let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
                fill = parse_paint(&rest[..end]);
                rest = rest[end..].trim_start();
            } else if rest.starts_with("[[") {
                // Links are not drawn.
                let end = rest.find("]]").map_or(rest.len(), |e| e + 2);
                rest = rest[end..].trim_start();
            } else {
                return err(l.no, format!("cannot understand \"{rest}\" in the {} declaration", kind.keyword()));
            }
        }
        let idx = self.ensure(&id);
        let d = &mut self.decls[idx];
        d.kind = kind;
        d.display = display;
        if stereotype.is_some() {
            d.stereotype = stereotype;
        }
        if fill.is_some() {
            d.fill = fill;
        }
        if order.is_some() {
            d.order = order;
        }
        if create {
            self.pending_create = Some((idx, l.no));
        }
        Ok(())
    }

    fn create(&mut self, l: &Line, rest: &str) -> Result<(), String> {
        let w = first_word(rest).to_ascii_lowercase();
        if let Some(kind) = PKind::from_word(&w) {
            return self.declare(l, kind, rest_after_word(rest), true);
        }
        let Some((name, _)) = read_name(rest) else { return err(l.no, "`create` needs a participant name") };
        let idx = self.ensure(&name);
        self.pending_create = Some((idx, l.no));
        Ok(())
    }

    fn life_event(&mut self, l: &Line, word: &str, rest: &str) -> Result<(), String> {
        let Some((name, after)) = read_name(rest) else { return err(l.no, format!("`{word}` needs a participant name")) };
        let who = self.ensure(&name);
        let color = after.trim().strip_prefix('#').and_then(|c| parse_paint(&format!("#{c}")));
        let kind = match word {
            "activate" => EffectKind::Activate,
            "deactivate" => EffectKind::Deactivate,
            _ => EffectKind::Destroy,
        };
        let effect = Effect { kind, who, color };
        if kind == EffectKind::Activate {
            if let Some((from, _)) = self.last_message {
                self.returns.push((from, who));
            }
        } else if kind == EffectKind::Deactivate {
            if let Some(i) = self.returns.iter().rposition(|&(_, c)| c == who) {
                self.returns.remove(i);
            }
        }
        // Attach to the message right before, so the arrow lands on the bar.
        if let Some(Event::Message(m)) = self.events.last_mut() {
            let touches = |e: Endpoint| e == Endpoint::Part(who);
            if touches(m.from) || touches(m.to) {
                m.effects.push(effect);
                return Ok(());
            }
        }
        self.events.push(Event::Effect(effect));
        Ok(())
    }

    fn ret(&mut self, l: &Line, rest: &str) -> Result<(), String> {
        let (caller, callee) = match self.returns.pop() {
            Some(x) => x,
            None => match self.last_message {
                Some((Endpoint::Part(a), Endpoint::Part(b))) => (Endpoint::Part(b), a),
                _ => return err(l.no, "`return` has no message to return from"),
            },
        };
        let label = self.ctx.reg.add_opt(self.message_runs(rest), self.wrap_width());
        let label = self.with_number(label, rest);
        self.events.push(Event::Message(Message {
            from: Endpoint::Part(callee),
            to: caller,
            arrow: Arrow { dashed: true, color: None, thickness: None, hidden: false, head_start: Head::None, head_end: Head::Filled },
            label,
            effects: vec![Effect { kind: EffectKind::Deactivate, who: callee, color: None }],
            creates: false,
            line: l.no,
        }));
        self.last_message = Some((Endpoint::Part(callee), caller));
        Ok(())
    }

    /// Registers the message label, with the autonumber prefix when active. The label
    /// must be registered here (not before) so the numbering follows source order.
    fn with_number(&mut self, plain: Option<LabelId>, text: &str) -> Option<LabelId> {
        let nums = self.number_runs();
        if nums.is_empty() {
            return plain;
        }
        // Replace the label registered a moment ago by one that includes the number.
        if plain.is_some() {
            self.ctx.reg.labels.pop();
        }
        let mut runs = nums;
        runs.extend(self.message_runs(text));
        Some(self.ctx.reg.add(runs, self.wrap_width()))
    }

    fn autonumber(&mut self, l: &Line, rest: &str) -> Result<(), String> {
        let mut tokens: Vec<String> = Vec::new();
        let mut r = rest.trim();
        while !r.is_empty() {
            if let Some(q) = r.strip_prefix('"') {
                let end = q.find('"').unwrap_or(q.len());
                tokens.push(format!("\"{}", &q[..end]));
                r = q[(end + 1).min(q.len())..].trim_start();
            } else {
                let end = r.find(char::is_whitespace).unwrap_or(r.len());
                tokens.push(r[..end].to_string());
                r = r[end..].trim_start();
            }
        }
        let first = tokens.first().map(|t| t.to_ascii_lowercase());
        match first.as_deref() {
            Some("stop") => {
                if let Some(a) = self.auto.as_mut() {
                    a.active = false;
                }
                return Ok(());
            }
            Some("resume") => {
                let Some(a) = self.auto.as_mut() else { return err(l.no, "`autonumber resume` without a running autonumber") };
                a.active = true;
                for t in &tokens[1..] {
                    if let Some(f) = t.strip_prefix('"') {
                        a.format = Some(f.to_string());
                    } else if let Ok(n) = t.parse::<i64>() {
                        a.step = n;
                    }
                }
                return Ok(());
            }
            _ => {}
        }
        let mut a = Autonumber { next: 1, step: 1, format: None, active: true };
        let mut nums = 0;
        for t in &tokens {
            if let Some(f) = t.strip_prefix('"') {
                a.format = Some(f.to_string());
            } else if let Ok(n) = t.parse::<i64>() {
                if nums == 0 {
                    a.next = n;
                } else {
                    a.step = n;
                }
                nums += 1;
            } else {
                return err(l.no, format!("cannot understand \"{t}\" in autonumber"));
            }
        }
        self.auto = Some(a);
        Ok(())
    }

    fn group_cond(&mut self, text: &str) -> LabelId {
        let mut base = self.base(&["sequencegroup", "sequencegroupheader"], 11.0);
        base.bold = true;
        let inner = format!("[{text}]");
        self.ctx.reg.add(creole::runs(&inner, &base), None)
    }

    fn group_start(&mut self, l: &Line, word: &str, rest: &str) -> Result<(), String> {
        let mut base = self.base(&["sequencegroupheader"], 13.0);
        base.bold = true;
        let tab = self.ctx.reg.add(creole::runs(word, &base), None);
        let mut cond_text = rest.to_string();
        // `group Title [Subtitle]`: the title takes the tab.
        let mut tab = tab;
        if word == "group" {
            if let Some(open) = rest.find('[') {
                if rest.ends_with(']') {
                    let title = rest[..open].trim();
                    cond_text = rest[open + 1..rest.len() - 1].to_string();
                    if !title.is_empty() {
                        self.ctx.reg.labels.pop();
                        tab = self.ctx.reg.add(creole::runs(title, &base), None);
                    }
                }
            } else if !rest.is_empty() {
                self.ctx.reg.labels.pop();
                tab = self.ctx.reg.add(creole::runs(rest, &base), None);
                cond_text.clear();
            }
        }
        let cond = (!cond_text.is_empty()).then(|| self.group_cond(&cond_text));
        self.events.push(Event::GroupStart { tab, cond });
        crate::plantuml::lexer::check_nesting(self.groups.len(), l.no)?;
        self.groups.push(l.no);
        Ok(())
    }

    fn end(&mut self, l: &Line, rest: &str) -> Result<(), String> {
        if rest.eq_ignore_ascii_case("box") {
            if self.open_box.take().is_none() {
                return err(l.no, "`end box` without a box");
            }
            return Ok(());
        }
        if self.groups.pop().is_none() {
            return err(l.no, "`end` without a group to close");
        }
        self.events.push(Event::GroupEnd);
        Ok(())
    }

    fn box_start(&mut self, l: &Line, rest: &str) -> Result<(), String> {
        if self.open_box.is_some() {
            return err(l.no, "boxes cannot be nested");
        }
        let mut name = rest.trim();
        let mut fill = None;
        if let Some(pos) = name.rfind('#') {
            if !name[pos..].contains('"') {
                fill = parse_paint(name[pos..].trim());
                name = name[..pos].trim();
            }
        }
        let name = unquote(name);
        let base = self.base(&["sequencebox"], 13.0);
        let label = if name.is_empty() { None } else { self.ctx.reg.add_opt(creole::runs(name, &base), None) };
        self.boxes.push(BoxDecl { label, fill });
        self.open_box = Some(self.boxes.len() - 1);
        Ok(())
    }

    fn note(&mut self, l: &Line, text: &str, cur: &mut Cursor) -> Result<(), String> {
        let t = text.trim_start_matches('/').trim_start();
        let kw = first_word(t).to_ascii_lowercase();
        let kind = match kw.as_str() {
            "hnote" => NoteKind::Hex,
            "rnote" => NoteKind::Rect,
            _ => NoteKind::Note,
        };
        let mut rest = rest_after_word(t);
        let word_len = rest.find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(rest.len());
        let pos_word = rest[..word_len].to_ascii_lowercase();
        rest = rest[word_len..].trim_start();
        let place = match pos_word.as_str() {
            "left" | "right" => {
                let after_of = if first_word(rest).eq_ignore_ascii_case("of") { rest_after_word(rest) } else { rest };
                if after_of.is_empty() || after_of.starts_with(':') || after_of.starts_with('#') {
                    rest = after_of;
                    NotePlace::OnMessage { right: pos_word == "right" }
                } else {
                    let (name, r) = match read_name(after_of) {
                        Some(x) => x,
                        None => return err(l.no, "the note needs a participant after `of`"),
                    };
                    rest = r;
                    let p = self.ensure(&name);
                    if pos_word == "left" { NotePlace::Left(p) } else { NotePlace::Right(p) }
                }
            }
            "over" => {
                let mut names = Vec::new();
                let mut r = rest;
                loop {
                    let Some((name, after)) = read_name(r) else { return err(l.no, "the note needs a participant after `over`") };
                    names.push(self.ensure(&name));
                    r = after.trim_start();
                    match r.strip_prefix(',') {
                        Some(more) => r = more,
                        None => break,
                    }
                }
                rest = r;
                NotePlace::Over(names)
            }
            "across" => NotePlace::Across,
            _ => return err(l.no, format!("cannot understand the note \"{text}\"")),
        };
        let rest = rest.trim();
        let (fill, body_start) = match rest.strip_prefix('#') {
            Some(r) => {
                let end = r.find(|c: char| c.is_whitespace() || c == ':').unwrap_or(r.len());
                (parse_paint(&r[..end]), r[end..].trim())
            }
            None => (None, rest),
        };
        let body = if let Some(b) = body_start.strip_prefix(':') {
            b.trim().to_string()
        } else if body_start.is_empty() {
            let lines = cur.block(l.no, "this note", |t| is_end(t, &["note", "hnote", "rnote"]))?;
            lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n")
        } else {
            return err(l.no, format!("cannot understand \"{body_start}\" in the note"));
        };
        if let NotePlace::OnMessage { .. } = place {
            if !matches!(self.events.last(), Some(Event::Message(_))) {
                return err(l.no, "a note without `of` or `over` has to follow a message");
            }
        }
        let base = self.base(&["note", "sequencenote"], 13.0);
        let Some(label) = self.ctx.reg.add_opt(creole::runs(&body, &base), None) else { return err(l.no, "the note is empty") };
        self.events.push(Event::Note(Note { kind, place, fill, label }));
        Ok(())
    }

    fn reference(&mut self, l: &Line, cur: &mut Cursor) -> Result<(), String> {
        let rest = rest_after_word(rest_after_word(&l.text));
        let mut names = Vec::new();
        let mut r = rest;
        loop {
            let Some((name, after)) = read_name(r) else { return err(l.no, "`ref over` needs participants") };
            names.push(self.ensure(&name));
            r = after.trim_start();
            match r.strip_prefix(',') {
                Some(more) => r = more,
                None => break,
            }
        }
        let body = if let Some(b) = r.strip_prefix(':') {
            b.trim().to_string()
        } else if r.is_empty() {
            let lines = cur.block(l.no, "this ref", |t| is_end(t, &["ref"]))?;
            lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n")
        } else {
            return err(l.no, format!("cannot understand \"{r}\" in the ref"));
        };
        let mut tabbase = self.base(&["sequencegroupheader"], 13.0);
        tabbase.bold = true;
        let tab = self.ctx.reg.add(creole::runs("ref", &tabbase), None);
        let base = self.base(&["sequencereference"], 13.0);
        let label = self.ctx.reg.add(creole::runs(&body, &base), None);
        self.events.push(Event::Ref { over: names, label, tab });
        Ok(())
    }

    fn message(&mut self, l: &Line) -> Result<(), String> {
        let text = l.text.as_str();
        let Some(tok) = find_arrow(text) else { return err(l.no, format!("syntax error, cannot understand \"{text}\"")) };
        let left_txt = &text[..tok.start];
        let right_txt = &text[tok.end..];

        // The target name stops before an activation shorthand, which precedes `: text`.
        let (target_txt, mut tail) = {
            let rt = right_txt.trim_start();
            if rt.starts_with(']') || (rt.starts_with('?') && !rt[1..].starts_with(|c: char| c.is_alphanumeric())) {
                (&rt[..1], &rt[1..])
            } else {
                match read_name(rt) {
                    Some((_, after)) => (&rt[..rt.len() - after.len()], after),
                    None => return err(l.no, "the arrow has no target"),
                }
            }
        };
        let (from_tok, to_tok, head_start, head_end) = match (tok.left, tok.right) {
            (Head::None, r) => (left_txt, target_txt, Head::None, r),
            (lh, Head::None) => (target_txt, left_txt, Head::None, lh),
            (lh, rh) => (left_txt, target_txt, lh, rh),
        };
        // For `<-` the source is on the right, so the sides swap.
        let reversed = tok.right == Head::None && tok.left != Head::None;
        let from = self.endpoint(from_tok, l.no, !reversed)?;
        let to = self.endpoint(to_tok, l.no, reversed)?;
        if from == Endpoint::Left && to == Endpoint::Right || (matches!(from, Endpoint::Left | Endpoint::Right) && from == to) {
            return err(l.no, "a message needs at least one participant");
        }

        let (dashed, color, thickness, hidden) = parse_arrow_attrs(tok.attrs.as_deref(), tok.dashes);
        let mut effects = Vec::new();
        let mut creates = false;
        tail = tail.trim_start();
        loop {
            let (kind, target_side) = if tail.starts_with("++") {
                (Some(EffectKind::Activate), true)
            } else if tail.starts_with("--") {
                (Some(EffectKind::Deactivate), false)
            } else if tail.starts_with("!!") {
                (Some(EffectKind::Destroy), true)
            } else if tail.starts_with("**") {
                creates = true;
                tail = tail[2..].trim_start();
                continue;
            } else {
                (None, true)
            };
            let Some(kind) = kind else { break };
            tail = tail[2..].trim_start();
            let mut color = None;
            if kind == EffectKind::Activate && tail.starts_with('#') {
                let end = tail.find(|c: char| c.is_whitespace() || c == ':').unwrap_or(tail.len());
                color = parse_paint(&tail[..end]);
                tail = tail[end..].trim_start();
            }
            let who = if target_side { to } else { from };
            if let Endpoint::Part(p) = who {
                effects.push(Effect { kind, who: p, color });
            }
        }
        let label_text = match tail.strip_prefix(':') {
            Some(t) => t.trim(),
            None if tail.is_empty() => "",
            None => return err(l.no, format!("cannot understand \"{tail}\" after the message target")),
        };

        for e in &effects {
            match e.kind {
                EffectKind::Activate => self.returns.push((from, e.who)),
                EffectKind::Deactivate => {
                    if let Some(i) = self.returns.iter().rposition(|&(_, c)| c == e.who) {
                        self.returns.remove(i);
                    }
                }
                EffectKind::Destroy => {}
            }
        }

        if let Some((p, line)) = self.pending_create {
            if to != Endpoint::Part(p) {
                return err(line, "`create` must be followed by a message to the created participant");
            }
            creates = true;
            self.pending_create = None;
        }

        let label = self.ctx.reg.add_opt(self.message_runs(label_text), self.wrap_width());
        let label = self.with_number(label, label_text);
        self.last_message = Some((from, to));
        self.events.push(Event::Message(Message {
            from,
            to,
            arrow: Arrow { dashed, color, thickness, hidden, head_start, head_end },
            label,
            effects,
            creates,
            line: l.no,
        }));
        Ok(())
    }

    fn finish(mut self) -> Result<Model, String> {
        // Participants sit in declaration order unless `order` says otherwise.
        let mut order: Vec<usize> = (0..self.decls.len()).collect();
        order.sort_by_key(|&i| (self.decls[i].order.unwrap_or(i as i64), i));
        let mut pos = vec![0; self.decls.len()];
        for (new, &old) in order.iter().enumerate() {
            pos[old] = new;
        }

        let mut participants = Vec::new();
        for &old in &order {
            let d = &self.decls[old];
            let kind_prefix = d.kind.keyword();
            let base = self.ctx.skin.text(&[&format!("sequence{kind_prefix}"), kind_prefix, "sequenceparticipant", "participant"], 14.0, "#000000");
            let mut runs = Vec::new();
            if let Some(st) = &d.stereotype {
                let mut sb = base.clone().italic();
                sb.size = (base.size * 0.86).round();
                runs.extend(creole::runs(&format!("\u{ab}{st}\u{bb}"), &sb));
                runs.push(Run::br());
            }
            runs.extend(creole::runs(&d.display, &base));
            let label = self.ctx.reg.add(runs, None);
            participants.push(Participant { id: d.id.clone(), kind: d.kind, label, fill: d.fill.clone(), box_idx: d.box_idx });
        }

        let map = |e: &mut Endpoint| {
            if let Endpoint::Part(p) = e {
                *p = pos[*p];
            }
        };
        let map_effect = |e: &mut Effect| e.who = pos[e.who];
        for ev in &mut self.events {
            match ev {
                Event::Message(m) => {
                    map(&mut m.from);
                    map(&mut m.to);
                    m.effects.iter_mut().for_each(map_effect);
                }
                Event::Note(n) => match &mut n.place {
                    NotePlace::Left(p) | NotePlace::Right(p) => *p = pos[*p],
                    NotePlace::Over(ps) => ps.iter_mut().for_each(|p| *p = pos[*p]),
                    _ => {}
                },
                Event::Ref { over, .. } => over.iter_mut().for_each(|p| *p = pos[*p]),
                Event::Effect(e) => map_effect(e),
                _ => {}
            }
        }
        if participants.is_empty() {
            return Err("line 1: the sequence diagram has no participants".to_string());
        }
        Ok(Model { participants, boxes: self.boxes, events: self.events, hide_footbox: self.hide_footbox })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(src: &str) -> Result<(Model, Ctx), String> {
        crate::plantuml::parse_with(src, parse)
    }

    fn msg(m: &Model, i: usize) -> &Message {
        m.events.iter().filter_map(|e| if let Event::Message(m) = e { Some(m) } else { None }).nth(i).unwrap()
    }

    #[test]
    fn arrows() {
        let tok = |s: &str| find_arrow(s).unwrap();
        assert_eq!((tok("A -> B").left, tok("A -> B").right), (Head::None, Head::Filled));
        assert_eq!(tok("A --> B").dashes, 2);
        assert_eq!(tok("A ->> B").right, Head::Open);
        assert_eq!(tok("A -\\ B").right, Head::HalfLower);
        assert_eq!(tok("A -/ B").right, Head::HalfUpper);
        assert_eq!(tok("A ->x B").right, Head::Cross);
        assert_eq!(tok("A ->o B").right, Head::Circle);
        assert_eq!(tok("A <- B").left, Head::Filled);
        assert_eq!(tok("A <-> B").left, Head::Filled);
        assert_eq!(tok("A o-> B").left, Head::Circle);
        assert_eq!(tok("[-> B").left, Head::None);
        assert_eq!(tok("A -[#red]-> B").attrs.as_deref(), Some("#red"));
        assert!(find_arrow("my-name and more").is_none());
        assert_eq!(tok("\"a->b\" -> C").right, Head::Filled);
    }

    #[test]
    fn participants_and_messages() {
        let (m, _) = model("participant \"Long Name\" as L #red\nactor Bob\nL -> Bob : hi\nBob --> L : back\nL -> Carol").unwrap();
        assert_eq!(m.participants.len(), 3);
        assert_eq!(m.participants[0].id, "L");
        assert_eq!(m.participants[1].kind, PKind::Actor);
        assert!(m.participants[0].fill.is_some());
        assert!(msg(&m, 1).arrow.dashed);
        assert_eq!(msg(&m, 1).from, Endpoint::Part(1));
    }

    #[test]
    fn order_keyword() {
        let (m, _) = model("participant A order 30\nparticipant B order 10\nA -> B").unwrap();
        assert_eq!(m.participants[0].id, "B");
        assert_eq!(msg(&m, 0).from, Endpoint::Part(1));
    }

    #[test]
    fn edge_messages_and_direction() {
        let (m, _) = model("[-> A : in\nA ->] : out\nA <- B").unwrap();
        assert_eq!(msg(&m, 0).from, Endpoint::Left);
        assert_eq!(msg(&m, 1).to, Endpoint::Right);
        assert_eq!(msg(&m, 2).from, Endpoint::Part(1));
        assert_eq!(msg(&m, 2).to, Endpoint::Part(0));
    }

    #[test]
    fn shorthand_activation_and_return() {
        let (m, _) = model("A -> B ++ : call\nB -> C ++\nreturn x\nreturn y").unwrap();
        assert_eq!(msg(&m, 0).effects.len(), 1);
        let r = msg(&m, 2);
        assert_eq!((r.from, r.to), (Endpoint::Part(2), Endpoint::Part(1)));
        assert_eq!(r.effects[0].kind, EffectKind::Deactivate);
        let r2 = msg(&m, 3);
        assert_eq!((r2.from, r2.to), (Endpoint::Part(1), Endpoint::Part(0)));
    }

    #[test]
    fn activate_line_attaches_to_previous_message() {
        let (m, _) = model("A -> B\nactivate B\nnote right of B : x\ndeactivate B").unwrap();
        assert_eq!(msg(&m, 0).effects.len(), 1);
        assert!(matches!(m.events.last(), Some(Event::Effect(_))));
    }

    #[test]
    fn groups_and_notes() {
        let (m, _) = model("alt ok\nA -> B\nelse bad\nA -> C\nend\nnote over A, B\nline1\nline2\nend note").unwrap();
        assert!(matches!(m.events[0], Event::GroupStart { .. }));
        assert!(matches!(m.events.last(), Some(Event::Note(Note { place: NotePlace::Over(_), .. }))));
        assert!(model("alt x\nA -> B").unwrap_err().contains("line 1"));
        assert!(model("A -> B\nend").unwrap_err().starts_with("line 2:"));
    }

    #[test]
    fn autonumber_prefixes_labels() {
        let (_, ctx) = model("autonumber 10 5\nA -> B : x\nA -> B : y").unwrap();
        let texts: Vec<String> = ctx.reg.labels.iter().map(|l| creole::plain(&l.runs)).collect();
        assert!(texts.contains(&"10 x".to_string()), "{texts:?}");
        assert!(texts.contains(&"15 y".to_string()), "{texts:?}");
    }

    #[test]
    fn syntax_errors_carry_line_numbers() {
        let e = model("A -> B\nthis is nonsense").unwrap_err();
        assert!(e.starts_with("line 2:"), "{e}");
        let e = model("A -> B\nA -> \"B").unwrap_err();
        assert!(e.starts_with("line 2:"), "{e}");
    }

    #[test]
    fn label_order_is_deterministic() {
        let src = "title T\nparticipant A\nA -> B : one\nnote left of A : n\nalt c\nB --> A : two\nend";
        let a = model(src).unwrap().1.reg.labels;
        let b = model(src).unwrap().1.reg.labels;
        assert_eq!(a, b);
    }

    #[test]
    fn create_requires_a_message() {
        assert!(model("create B\nA -> C").unwrap_err().starts_with("line 1:"));
        let (m, _) = model("create B\nA -> B").unwrap();
        assert!(msg(&m, 0).creates);
    }
}
