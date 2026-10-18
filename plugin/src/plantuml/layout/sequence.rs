//! Sequence diagram layout, in the spirit of PlantUML's own: participants sit in
//! columns, the distance between columns is the largest of everything that has to fit
//! between them (box widths, message labels, notes, group headers), and a single
//! vertical cursor moves down through the events.
//!
//! Columns: index 0 is the left edge of the diagram (target of `[->` messages), columns
//! `1..=n` are the participants and `n + 1` is the right edge. Every width requirement
//! is a constraint `x[b] - x[a] >= d` (a < b); the positions are the smallest ones that
//! satisfy all of them. The requirements do not depend on the positions, so the diagram
//! is drawn once to collect them, solved, and drawn again until nothing moves
//! (normally two passes).
//!
//! Drawing order is kept in layers so that lifelines sit under activation bars, which
//! sit under arrows and text, no matter in which order the events produce them.

use std::collections::BTreeMap;

use super::shapes::{anchor_left, arrow, draw_icon, dashed, icon_size, note_shape, solid, IconKind, NoteKind, ARROW_DASH, DOTTED};
use super::Body;
use crate::plantuml::diagrams::sequence::*;
use crate::plantuml::model::{LabelId, Measured};
use crate::plantuml::skin::{self, Skin};
use crate::scene::{HAlign, Paint, PathBuilder, Scene, Stroke, Style, VAlign};

const LOOP_W: f32 = 36.0;
const LOOP_H: f32 = 14.0;
const BAR_W: f32 = 10.0;
const BAR_STEP: f32 = 5.0;
const HEAD_PAD_X: f32 = 14.0;
const HEAD_PAD_Y: f32 = 14.0;
const GROUP_MARGIN: f32 = 8.0;

fn right_edge(level: usize) -> f32 {
    BAR_STEP * level as f32
}

fn left_edge(level: usize) -> f32 {
    if level == 0 { 0.0 } else { BAR_STEP * level as f32 - 2.0 * BAR_STEP }
}

type Reqs = BTreeMap<(usize, usize), f32>;

fn add_req(reqs: &mut Reqs, a: usize, b: usize, d: f32) {
    debug_assert!(a < b, "requirement between {a} and {b}");
    let e = reqs.entry((a, b)).or_insert(0.0);
    if d > *e {
        *e = d;
    }
}

fn solve(cols: usize, reqs: &Reqs) -> Vec<f32> {
    let mut x = vec![0.0f32; cols];
    for j in 1..cols {
        x[j] = x[j - 1] + 1.0;
        for (&(a, b), &d) in reqs.range((0, 0)..) {
            if b == j {
                x[j] = x[j].max(x[a] + d);
            }
        }
    }
    x
}

#[derive(Default)]
struct Layers {
    bg: Scene,
    /// Between the boxes (`bg`) and the lifelines, so a box fill never hides a highlight.
    hl: Scene,
    life: Scene,
    frame: Scene,
    act: Scene,
    top: Scene,
}

impl Default for Scene {
    fn default() -> Self {
        Scene::new(0.0, 0.0)
    }
}

/// A horizontal extent expressed as column plus offset, so group frames can state their
/// width requirements without knowing the final positions.
#[derive(Clone, Copy)]
struct Ext {
    x: f32,
    col: usize,
    off: f32,
}

struct OpenGroup {
    tab: LabelId,
    cond: Option<LabelId>,
    top: f32,
    lo: Option<Ext>,
    hi: Option<Ext>,
    elses: Vec<(f32, Option<LabelId>)>,
    /// Set for a highlight region, which has no frame, tab or text: its fill and the
    /// position in the highlight layer it must be drawn at.
    fill: Option<(Paint, usize)>,
}

struct Bar {
    start: f32,
    fill: Option<Paint>,
}

struct Style_ {
    lifeline: String,
    arrow: String,
    arrow_w: f32,
    act_fill: Paint,
    act_border: String,
    group_border: String,
    group_tab_fill: Paint,
    note_fill: Paint,
    note_border: String,
    align: HAlign,
    below: bool,
}

struct D<'a> {
    m: &'a Model,
    skin: &'a Skin,
    meas: &'a Measured,
    xs: &'a [f32],
    st: Style_,
    reqs: Reqs,
    layers: Layers,
    n: usize,
    head: Vec<(f32, f32)>,
    created: Vec<bool>,
    cursor: f32,
    levels: Vec<Vec<Bar>>,
    /// y at which each participant's lifeline starts and ends.
    life_start: Vec<f32>,
    life_end: Vec<Option<f32>>,
    delays: Vec<(f32, f32)>,
    groups: Vec<OpenGroup>,
    min_x: f32,
    max_x: f32,
    last_msg: Option<LastMsg>,
    /// Geometry of every message drawn so far, for the tests.
    messages: Vec<MsgInfo>,
    top_heads: Vec<(f32, f32)>,
    created_heads: Vec<(usize, f32)>,
    act_levels: Vec<Scene>,
}

struct LastMsg {
    top: f32,
    lo: usize,
    hi: usize,
    lo_x: f32,
    hi_x: f32,
}

impl<'a> D<'a> {
    fn x(&self, p: usize) -> f32 {
        self.xs[p + 1]
    }

    fn col_of(&self, e: Endpoint) -> usize {
        match e {
            Endpoint::Part(p) => p + 1,
            Endpoint::Left => 0,
            Endpoint::Right => self.n + 1,
        }
    }

    fn level(&self, p: usize) -> usize {
        self.levels[p].len()
    }

    fn req(&mut self, a: usize, b: usize, d: f32) {
        if a < b {
            add_req(&mut self.reqs, a, b, d);
        }
    }

    fn bounds(&mut self, x0: f32, x1: f32) {
        self.min_x = self.min_x.min(x0);
        self.max_x = self.max_x.max(x1);
    }

    fn extent(&mut self, lo: Ext, hi: Ext) {
        self.bounds(lo.x, hi.x);
        if let Some(g) = self.groups.last_mut() {
            if g.lo.is_none_or(|e| lo.x < e.x) {
                g.lo = Some(lo);
            }
            if g.hi.is_none_or(|e| hi.x > e.x) {
                g.hi = Some(hi);
            }
        }
    }

    fn ext(&self, col: usize, off: f32) -> Ext {
        Ext { x: self.xs[col] + off, col, off }
    }

    fn text_size(&self, id: Option<LabelId>) -> (f32, f32) {
        id.map_or((0.0, 0.0), |i| self.meas.size(i))
    }

    // ---- participants ------------------------------------------------------------

    /// Where a creation arrow lands: the icon or box border, at the icon's middle.
    fn create_half_width(&self, p: usize) -> f32 {
        match self.m.participants[p].kind {
            PKind::Participant | PKind::Collections | PKind::Queue => self.head[p].0 / 2.0,
            k => icon_size(icon_of(k)).0 / 2.0,
        }
    }

    fn create_anchor_y(&self, p: usize) -> f32 {
        match self.m.participants[p].kind {
            PKind::Participant | PKind::Collections | PKind::Queue => self.head[p].1 / 2.0,
            k => icon_size(icon_of(k)).1 / 2.0,
        }
    }

    fn group_stroke(&self, border: &str) -> Stroke {
        let st = &self.skin.style;
        match &st.group_dash {
            Some(d) => dashed(border, st.group_width, d),
            None => solid(border, st.group_width),
        }
    }

    fn fill_of(&self, p: &Participant) -> Paint {
        let k = p.kind.keyword();
        p.fill.clone().unwrap_or_else(|| self.skin.fill(&[&format!("sequence{k}"), k, "sequenceparticipant", "participant"], skin::PARTICIPANT_FILL))
    }

    fn border_of(&self, p: &Participant) -> String {
        let k = p.kind.keyword();
        self.skin.border(&[&format!("sequence{k}"), k, "sequenceparticipant", "participant"], skin::BORDER)
    }

    /// Draws a participant head with its top at `top`. `x` is the lifeline.
    fn draw_head(&mut self, pi: usize, x: f32, top: f32) {
        let p = &self.m.participants[pi];
        let (w, h) = self.head[pi];
        let fill = self.fill_of(p);
        let border = self.border_of(p);
        let k = p.kind.keyword();
        let radius = self.skin.radius(&[&format!("sequence{k}"), k, "sequenceparticipant", "participant"], 5.0);
        let pw = self.skin.style.participant_width;
        let sty = || Style::new().fill(fill.clone()).stroke(solid(&border, pw));
        let left = x - w / 2.0;
        let (tw, th) = self.meas.size(p.label);
        let sc = &mut self.layers.top;
        match p.kind {
            PKind::Participant => {
                sc.push_rect(left, top, w, h, radius, sty());
                self.meas.text_in(sc, p.label, left, top, w, h, HAlign::Center, VAlign::Middle);
            }
            PKind::Collections => {
                sc.push_rect(left + 4.0, top, w - 4.0, h - 4.0, radius, sty());
                sc.push_rect(left, top + 4.0, w - 4.0, h - 4.0, radius, sty());
                self.meas.text_in(sc, p.label, left, top + 4.0, w - 4.0, h - 4.0, HAlign::Center, VAlign::Middle);
            }
            PKind::Queue => {
                let c = 8.0;
                let kk = 0.5523;
                let (l, r, t, b, my) = (left, left + w, top, top + h, top + h / 2.0);
                let ry = h / 2.0;
                let mut path = PathBuilder::new();
                path.move_to(l + c, t);
                path.line_to(r - c, t);
                path.cubic_to(r - c + c * kk, t, r, my - ry * kk, r, my);
                path.cubic_to(r, my + ry * kk, r - c + c * kk, b, r - c, b);
                path.line_to(l + c, b);
                path.cubic_to(l + c - c * kk, b, l, my + ry * kk, l, my);
                path.cubic_to(l, my - ry * kk, l + c - c * kk, t, l + c, t);
                path.close();
                sc.push_path(path, sty());
                let mut lid = PathBuilder::new();
                lid.move_to(r - c, t);
                lid.cubic_to(r - c - c * kk, t, r - 2.0 * c, my - ry * kk, r - 2.0 * c, my);
                lid.cubic_to(r - 2.0 * c, my + ry * kk, r - c - c * kk, b, r - c, b);
                sc.push_path(lid, Style::new().stroke(solid(&border, pw)));
                self.meas.text_in(sc, p.label, l + c, t, w - 3.0 * c, h, HAlign::Center, VAlign::Middle);
            }
            kind => {
                let ik = icon_of(kind);
                let (_, ih) = icon_size(ik);
                draw_icon(sc, ik, x, top, &fill, &border, pw);
                self.meas.text_at(sc, p.label, x, top + ih + 3.0, HAlign::Center);
                let _ = (tw, th);
            }
        }
        self.bounds(left, left + w);
    }

    // ---- bars ----------------------------------------------------------------------

    fn open_bar(&mut self, p: usize, y: f32, fill: Option<Paint>) {
        self.levels[p].push(Bar { start: y, fill });
    }

    fn close_bar(&mut self, p: usize, y: f32) {
        let Some(bar) = self.levels[p].pop() else { return };
        let k = self.levels[p].len();
        let x = self.x(p) + BAR_STEP * k as f32 - BAR_W / 2.0;
        let fill = bar.fill.unwrap_or_else(|| self.st.act_fill.clone());
        // Outer bars are drawn first so nested ones stay visible on top of them.
        while self.act_levels.len() <= k {
            self.act_levels.push(Scene::default());
        }
        self.act_levels[k].push_rect(x, bar.start, BAR_W, (y - bar.start).max(2.0), 0.0, Style::new().fill(fill).stroke(solid(&self.st.act_border, 1.0)));
        self.bounds(x, x + BAR_W);
    }

    fn destroy_mark(&mut self, p: usize, y: f32) {
        let x = self.x(p) + BAR_STEP * self.level(p).saturating_sub(1) as f32 * 0.0;
        let r = 8.0;
        let s = solid(&self.st.arrow, 2.0);
        self.layers.top.push_polyline(&[(x - r, y - r), (x + r, y + r)], s.clone());
        self.layers.top.push_polyline(&[(x - r, y + r), (x + r, y - r)], s);
        while self.level(p) > 0 {
            self.close_bar(p, y);
        }
        self.life_end[p] = Some(y);
    }

    fn apply_effects(&mut self, effects: &[Effect], y: f32, skip_open: bool) {
        for e in effects {
            match e.kind {
                EffectKind::Activate => {
                    if !skip_open {
                        self.open_bar(e.who, y, e.color.clone());
                    }
                }
                EffectKind::Deactivate => self.close_bar(e.who, y),
                EffectKind::Destroy => self.destroy_mark(e.who, y),
            }
        }
    }

    // ---- messages --------------------------------------------------------------------

    fn message(&mut self, msg: &Message) {
        let (tw, th) = self.text_size(msg.label);
        let has_label = msg.label.is_some();
        let top = self.cursor;

        let activates = |who: Endpoint| msg.effects.iter().filter(|e| e.kind == EffectKind::Activate && Endpoint::Part(e.who) == who).count();
        let level_of = |me: &Self, e: Endpoint| match e {
            Endpoint::Part(p) => me.level(p) + activates(e),
            _ => 0,
        };
        let (lv_from, lv_to) = (level_of(self, msg.from), level_of(self, msg.to));
        let (cf, ct) = (self.col_of(msg.from), self.col_of(msg.to));
        let color = msg.arrow.color.clone().map(|c| crate::scene::hex(&c)).unwrap_or_else(|| self.st.arrow.clone());
        let width = msg.arrow.thickness.unwrap_or(self.st.arrow_w);
        let stroke = if msg.arrow.dashed { dashed(&color, width, &ARROW_DASH) } else { solid(&color, width) };

        // Self message.
        if cf == ct {
            let Endpoint::Part(p) = msg.from else { return };
            let x0 = self.x(p) + right_edge(lv_from.max(lv_to));
            let label_h = if has_label { th + 2.0 } else { 0.0 };
            let y1 = top + 3.0 + label_h + if has_label { 6.0 } else { 7.0 };
            let y2 = y1 + LOOP_H;
            let ext_r = LOOP_W.max(tw + 7.0);
            self.req(cf, cf + 1, right_edge(lv_from.max(lv_to)) + ext_r + 10.0);
            let lo = self.ext(cf, right_edge(lv_from.max(lv_to)));
            let hi = self.ext(cf, right_edge(lv_from.max(lv_to)) + ext_r);
            self.extent(lo, hi);
            if !msg.arrow.hidden {
                let (hs, he) = (msg.arrow.head_start, msg.arrow.head_end);
                let pts = [(x0, y1), (x0 + LOOP_W, y1), (x0 + LOOP_W, y2), (x0, y2)];
                arrow(&mut self.layers.top, &pts, hs, he, stroke);
                if let Some(id) = msg.label {
                    self.meas.text_at(&mut self.layers.top, id, x0 + 7.0, y1 - 7.0 - th, HAlign::Left);
                }
            }
            let lbl = msg.label.map(|_| (x0 + 7.0, x0 + 7.0 + tw));
            self.messages.push(MsgInfo { y: y1, from_x: x0, to_x: x0 + LOOP_W, label: lbl, is_self: true });
            self.finish_message(msg, y1, top, (p, p));
            self.cursor = y2 + 8.0;
            self.apply_effects(&msg.effects, y1, true);
            self.start_effects(msg, y1);
            return;
        }

        let ltr = ct > cf;
        let (off_s, off_d) = {
            let side = |role_src: bool, e: Endpoint, lv: usize| -> f32 {
                match e {
                    Endpoint::Left => 6.0,
                    Endpoint::Right => -6.0,
                    Endpoint::Part(_) => {
                        // Leaving to the right starts at the right edge of the bar;
                        // arriving from the left ends at its left edge.
                        let right_side = if role_src { ltr } else { !ltr };
                        if right_side { right_edge(lv) } else { left_edge(lv) }
                    }
                }
            };
            (side(true, msg.from, lv_from), side(false, msg.to, lv_to))
        };

        // A created participant's box sits at the arrow; the arrow stops at its border.
        let mut off_d = off_d;
        if msg.creates {
            if let Endpoint::Part(p) = msg.to {
                let hw = self.create_half_width(p);
                off_d = if ltr { -hw } else { hw };
            }
        }

        let need = if has_label { tw + 14.0 } else { 20.0 };
        if ltr {
            self.req(cf, ct, need - off_d + off_s);
        } else {
            self.req(ct, cf, need - off_s + off_d);
        }

        let sx = self.xs[cf] + off_s;
        let dx = self.xs[ct] + off_d;
        let (lo_x, hi_x) = (sx.min(dx), sx.max(dx));
        let below = has_label && self.st.below && msg.arrow.dashed;
        let mut y = if !has_label {
            top + 12.0
        } else if below {
            top + 8.0
        } else {
            top + 3.0 + th + 8.0
        };
        let mut box_h = 0.0;
        if msg.creates {
            if let Endpoint::Part(p) = msg.to {
                box_h = self.head[p].1;
                y = y.max(top + self.create_anchor_y(p) + 2.0);
            }
        }
        let (lo_col, hi_col) = (cf.min(ct), cf.max(ct));
        let lo = self.ext(lo_col, if lo_col == cf { off_s } else { off_d });
        let hi = self.ext(hi_col, if hi_col == cf { off_s } else { off_d });
        self.extent(lo, hi);

        let mut label_range = None;
        if !msg.arrow.hidden {
            arrow(&mut self.layers.top, &[(sx, y), (dx, y)], msg.arrow.head_start, msg.arrow.head_end, stroke);
            if let Some(id) = msg.label {
                // Descenders (g, p, y) must not touch the line.
                let ty = if below { y + 4.0 } else { y - 7.0 - th };
                let (tx, anchor) = match self.st.align {
                    HAlign::Left => (lo_x + 7.0, HAlign::Left),
                    HAlign::Center => ((sx + dx) / 2.0, HAlign::Center),
                    HAlign::Right => (hi_x - 7.0, HAlign::Right),
                };
                self.meas.text_at(&mut self.layers.top, id, tx, ty, anchor);
                let x0 = anchor_left(anchor, tx, tw);
                label_range = Some((x0, x0 + tw));
            }
        }
        self.messages.push(MsgInfo { y, from_x: sx, to_x: dx, label: label_range, is_self: false });
        let (a, b) = match (msg.from, msg.to) {
            (Endpoint::Part(a), Endpoint::Part(b)) => (a, b),
            (Endpoint::Part(a), _) | (_, Endpoint::Part(a)) => (a, a),
            _ => (0, 0),
        };
        self.finish_message(msg, y, top, (a.min(b), a.max(b)));
        self.cursor = if below { y + 3.0 + th + 6.0 } else { y + 8.0 };
        if msg.creates {
            if let Endpoint::Part(p) = msg.to {
                let head_top = y - self.create_anchor_y(p);
                self.created_heads.push((p, head_top));
                self.life_start[p] = head_top + box_h;
                self.cursor = self.cursor.max(head_top + box_h + 6.0);
            }
        }
        self.apply_effects(&msg.effects, y, true);
        self.start_effects(msg, y);
    }

    /// Bars that start with the message begin at the arrow, after the arrow's end has
    /// been placed using the new level.
    fn start_effects(&mut self, msg: &Message, y: f32) {
        for e in &msg.effects {
            if e.kind == EffectKind::Activate {
                self.open_bar(e.who, y, e.color.clone());
            }
        }
    }

    fn finish_message(&mut self, msg: &Message, y: f32, top: f32, cols: (usize, usize)) {
        let _ = (msg, y);
        let (lo, hi) = cols;
        self.last_msg = Some(LastMsg { top, lo, hi, lo_x: self.x(lo), hi_x: self.x(hi) });
    }

    // ---- notes ------------------------------------------------------------------------

    fn note(&mut self, note: &Note) {
        let (tw, th) = self.meas.size(note.label);
        let extra = if note.kind == NoteKind::Hex { 8.0 } else { 0.0 };
        let (mut w, h) = (tw + if note.kind == NoteKind::Note { 21.0 } else { 14.0 } + extra, th + 10.0);
        let (n, cur) = (self.n, self.cursor);
        let mut top = cur + 3.0;
        let x;
        match &note.place {
            NotePlace::Left(p) | NotePlace::Right(p) => {
                let p = *p;
                let col = p + 1;
                let lv = self.level(p);
                if matches!(note.place, NotePlace::Right(_)) {
                    let off = right_edge(lv) + 5.0;
                    x = self.x(p) + off;
                    self.req(col, col + 1, off + w + 10.0);
                    let (lo, hi) = (self.ext(col, off), self.ext(col, off + w));
                    self.extent(lo, hi);
                } else {
                    let off = left_edge(lv) - 5.0;
                    x = self.x(p) + off - w;
                    self.req(col - 1, col, -off + w + 10.0);
                    let (lo, hi) = (self.ext(col, off - w), self.ext(col, off));
                    self.extent(lo, hi);
                }
            }
            NotePlace::Over(ps) => {
                let (lo_p, hi_p) = (*ps.iter().min().unwrap(), *ps.iter().max().unwrap());
                if lo_p == hi_p {
                    let c = lo_p + 1;
                    self.req(c - 1, c, w / 2.0 + 5.0);
                    self.req(c, c + 1, w / 2.0 + 5.0);
                    x = self.x(lo_p) - w / 2.0;
                    let (lo, hi) = (self.ext(c, -w / 2.0), self.ext(c, w / 2.0));
                    self.extent(lo, hi);
                } else {
                    let (c0, c1) = (lo_p + 1, hi_p + 1);
                    self.req(c0, c1, w - 20.0);
                    w = w.max(self.x(hi_p) - self.x(lo_p) + 20.0);
                    x = self.x(lo_p) - 10.0;
                    let (lo, hi) = (self.ext(c0, -10.0), self.ext(c1, 10.0));
                    self.extent(lo, hi);
                }
            }
            NotePlace::Across => {
                self.req(1, self.n, w - 20.0);
                w = w.max(self.x(self.n - 1) - self.x(0) + 20.0);
                x = self.x(0) - 10.0;
                let (lo, hi) = (self.ext(1, -10.0), self.ext(n, 10.0));
                self.extent(lo, hi);
            }
            NotePlace::OnMessage { right } => {
                let Some(l) = self.last_msg.as_ref() else { return };
                top = l.top + 3.0;
                let (lo, hi, lo_x, hi_x) = (l.lo, l.hi, l.lo_x, l.hi_x);
                if *right {
                    let col = hi + 1;
                    let off = right_edge(self.level(hi)) + 5.0;
                    x = hi_x + off;
                    self.req(col, col + 1, off + w + 10.0);
                    let (a, b) = (self.ext(col, off), self.ext(col, off + w));
                    self.extent(a, b);
                } else {
                    let col = lo + 1;
                    let off = left_edge(self.level(lo)) - 5.0;
                    x = lo_x + off - w;
                    self.req(col - 1, col, -off + w + 10.0);
                    let (a, b) = (self.ext(col, off - w), self.ext(col, off));
                    self.extent(a, b);
                }
            }
        }
        let fill = note.fill.clone().unwrap_or_else(|| self.st.note_fill.clone());
        let sc = &mut self.layers.top;
        note_shape(sc, note.kind, x, top, w, h, fill, &self.st.note_border, 1.0);
        let pad = if note.kind == NoteKind::Hex { 4.0 + 7.0 } else { 7.0 };
        self.meas.text_at(sc, note.label, x + pad, top + 5.0, HAlign::Left);
        self.cursor = (top + h + 5.0).max(self.cursor);
    }

    // ---- groups ------------------------------------------------------------------------

    fn group_start(&mut self, tab: LabelId, cond: Option<LabelId>) {
        let top = self.cursor + 2.0;
        let tab_h = self.meas.h(tab) + 4.0;
        self.cursor = top + tab_h + 4.0;
        self.groups.push(OpenGroup { tab, cond, top, lo: None, hi: None, elses: Vec::new(), fill: None });
    }

    fn group_else(&mut self, cond: Option<LabelId>) {
        let y = self.cursor + 2.0;
        let h = cond.map_or(0.0, |c| self.meas.h(c));
        self.cursor = y + h + 6.0;
        if let Some(g) = self.groups.last_mut() {
            g.elses.push((y, cond));
        }
    }

    fn highlight_start(&mut self, fill: &Paint) {
        let top = self.cursor + 2.0;
        self.cursor = top + 4.0;
        let slot = self.layers.hl.items.len();
        self.groups.push(OpenGroup { tab: LabelId(0), cond: None, top, lo: None, hi: None, elses: Vec::new(), fill: Some((fill.clone(), slot)) });
    }

    fn highlight_end(&mut self) {
        let Some(g) = self.groups.pop() else { return };
        let Some((fill, slot)) = g.fill else { return };
        let bottom = self.cursor + 2.0;
        let default = self.ext(1, 0.0);
        let (lo, hi) = (g.lo.unwrap_or(default), g.hi.unwrap_or(default));
        // Wider than a group frame so activation bars stay inside the region.
        let m = GROUP_MARGIN + 8.0;
        let (left, right) = (lo.x - m, hi.x + m);
        self.layers.hl.push_rect(left, g.top, right - left, bottom - g.top, 0.0, Style::new().fill(fill));
        // Nested regions end first but have to be drawn on top of the enclosing one.
        if let Some(rect) = self.layers.hl.items.pop() {
            self.layers.hl.items.insert(slot, rect);
        }
        self.bounds(left, right);
        self.cursor = bottom + 4.0;
        // An enclosing group has to contain this region too.
        self.extent(Ext { x: left, col: lo.col, off: lo.off - m }, Ext { x: right, col: hi.col, off: hi.off + m });
    }

    fn group_end(&mut self) {
        let Some(g) = self.groups.pop() else { return };
        let bottom = self.cursor + 2.0;
        let (tab_w, tab_h) = (self.meas.w(g.tab) + 18.0, self.meas.h(g.tab) + 4.0);
        let cond_w = g.cond.map_or(0.0, |c| self.meas.w(c) + 10.0);
        let mut needed = tab_w + cond_w + 12.0;
        for (_, c) in &g.elses {
            if let Some(c) = c {
                needed = needed.max(self.meas.w(*c) + 24.0);
            }
        }
        let default = self.ext(1, 0.0);
        let (lo, hi) = (g.lo.unwrap_or(default), g.hi.unwrap_or(default));
        let m = GROUP_MARGIN;
        let mut right = hi.x + m;
        let left = lo.x - m;
        if lo.col < hi.col {
            let have = (hi.x + m) - left;
            if have < needed {
                self.req(lo.col, hi.col, needed - 2.0 * m - (hi.off - lo.off));
            }
        } else {
            if right - left < needed {
                right = left + needed;
            }
            self.req(hi.col, hi.col + 1, (left + needed + m) - self.xs[hi.col]);
        }
        self.bounds(left, right);

        let border = self.st.group_border.clone();
        let (gw, frame) = (self.skin.style.group_width, self.group_stroke(&border));
        let fr = &mut self.layers.frame;
        fr.push_rect(left, g.top, right - left, bottom - g.top, 0.0, Style::new().stroke(frame));
        // Tab with the bottom right corner cut off.
        let cut = 6.0;
        fr.push_polygon(
            &[(left, g.top), (left + tab_w, g.top), (left + tab_w, g.top + tab_h - cut), (left + tab_w - cut, g.top + tab_h), (left, g.top + tab_h)],
            Style::new().fill(self.st.group_tab_fill.clone()).stroke(solid(&border, gw)),
        );
        self.meas.text_at(fr, g.tab, left + 8.0, g.top + 2.0, HAlign::Left);
        if let Some(c) = g.cond {
            let ch = self.meas.h(c);
            self.meas.text_at(fr, c, left + tab_w + 8.0, g.top + (tab_h - ch) / 2.0 + 1.0, HAlign::Left);
        }
        for (y, cond) in &g.elses {
            fr.push_polyline(&[(left, *y), (right, *y)], dashed(&border, 1.0, &[2.0, 2.0]));
            if let Some(c) = cond {
                self.meas.text_at(fr, *c, left + 8.0, *y + 2.0, HAlign::Left);
            }
        }
        self.cursor = bottom + 4.0;
        // The enclosing group has to contain this frame.
        let (lo2, hi2) = (Ext { x: left, col: lo.col, off: lo.off - m }, Ext { x: right, col: hi.col, off: hi.off + m });
        self.extent(lo2, hi2);
    }

    // ---- the rest -----------------------------------------------------------------------

    fn divider(&mut self, id: LabelId) {
        let (tw, th) = self.meas.size(id);
        let top = self.cursor + 6.0;
        let h = th + 8.0;
        let (x0, x1) = (self.xs[0], self.xs[self.n + 1]);
        let mid = top + h / 2.0;
        let line = solid(skin::BORDER, 1.0);
        self.layers.top.push_polyline(&[(x0, mid - 1.5), (x1, mid - 1.5)], line.clone());
        self.layers.top.push_polyline(&[(x0, mid + 1.5), (x1, mid + 1.5)], line);
        let w = tw + 24.0;
        let cx = (x0 + x1) / 2.0;
        let fill = self.skin.fill(&["sequencedivider"], skin::GROUP_TAB_FILL);
        self.layers.top.push_rect(cx - w / 2.0, top, w, h, 0.0, Style::new().fill(fill).stroke(solid(skin::BORDER, 1.5)));
        self.meas.text_at(&mut self.layers.top, id, cx, top + 4.0, HAlign::Center);
        self.cursor = top + h + 6.0;
        self.req(0, self.n + 1, w);
    }

    fn delay(&mut self, id: Option<LabelId>) {
        let th = id.map_or(0.0, |i| self.meas.h(i));
        let h = (th + 16.0).max(28.0);
        let top = self.cursor;
        self.delays.push((top, top + h));
        if let Some(id) = id {
            let cx = (self.xs[0] + self.xs[self.n + 1]) / 2.0;
            self.meas.text_at(&mut self.layers.top, id, cx, top + (h - th) / 2.0, HAlign::Center);
        }
        self.cursor = top + h;
    }

    fn reference(&mut self, over: &[usize], label: LabelId, tab: LabelId) {
        let (tw, th) = self.meas.size(label);
        let (tab_w, tab_h) = (self.meas.w(tab) + 18.0, self.meas.h(tab) + 4.0);
        let (lo_p, hi_p) = (*over.iter().min().unwrap(), *over.iter().max().unwrap());
        let (c0, c1) = (lo_p + 1, hi_p + 1);
        let w_need = (tw + 24.0).max(tab_w + 10.0);
        let m = 10.0;
        if c0 < c1 {
            self.req(c0, c1, w_need - 2.0 * m);
        } else {
            self.req(c0 - 1, c0, w_need / 2.0 + 5.0);
            self.req(c0, c0 + 1, w_need / 2.0 + 5.0);
        }
        let mut left = self.x(lo_p) - m;
        let mut right = self.x(hi_p) + m;
        if right - left < w_need {
            let mid = (left + right) / 2.0;
            left = mid - w_need / 2.0;
            right = mid + w_need / 2.0;
        }
        let top = self.cursor + 4.0;
        let h = tab_h + th + 8.0;
        let border = self.st.group_border.clone();
        let (gw, frame) = (self.skin.style.group_width, self.group_stroke(&border));
        let sc = &mut self.layers.top;
        sc.push_rect(left, top, right - left, h, 0.0, Style::new().fill(Paint::solid("#ffffff")).stroke(frame));
        let cut = 6.0;
        sc.push_polygon(
            &[(left, top), (left + tab_w, top), (left + tab_w, top + tab_h - cut), (left + tab_w - cut, top + tab_h), (left, top + tab_h)],
            Style::new().fill(self.st.group_tab_fill.clone()).stroke(solid(&border, gw)),
        );
        self.meas.text_at(sc, tab, left + 8.0, top + 2.0, HAlign::Left);
        self.meas.text_at(sc, label, (left + right) / 2.0, top + tab_h + 3.0, HAlign::Center);
        let (a, b) = (self.ext(c0, left - self.xs[c0]), self.ext(c1, right - self.xs[c1]));
        self.extent(a, b);
        self.cursor = top + h + 6.0;
    }

    fn run(&mut self) {
        for ev in &self.m.events {
            match ev {
                Event::Message(msg) => self.message(msg),
                Event::Note(n) => self.note(n),
                Event::Ref { over, label, tab } => self.reference(over, *label, *tab),
                Event::Divider(id) => self.divider(*id),
                Event::Delay(id) => self.delay(*id),
                Event::Space(h) => self.cursor += h,
                Event::GroupStart { tab, cond } => self.group_start(*tab, *cond),
                Event::GroupElse(c) => self.group_else(*c),
                Event::GroupEnd => self.group_end(),
                Event::Highlight(fill) => self.highlight_start(fill),
                Event::HighlightEnd => self.highlight_end(),
                Event::Effect(e) => {
                    let y = self.cursor;
                    self.apply_effects(std::slice::from_ref(e), y, false);
                    if e.kind == EffectKind::Destroy {
                        self.cursor += 10.0;
                    }
                }
            }
        }
    }
}

fn icon_of(k: PKind) -> IconKind {
    match k {
        PKind::Actor => IconKind::Actor,
        PKind::Boundary => IconKind::Boundary,
        PKind::Control => IconKind::Control,
        PKind::Entity => IconKind::Entity,
        _ => IconKind::Database,
    }
}

/// Geometry of a message after layout, for the invariants the tests check.
#[derive(Debug, Clone)]
pub struct MsgInfo {
    pub y: f32,
    pub from_x: f32,
    pub to_x: f32,
    /// Horizontal extent of the label text, if any.
    pub label: Option<(f32, f32)>,
    pub is_self: bool,
}

#[derive(Debug, Clone, Default)]
pub struct SeqInfo {
    /// Horizontal extent of every participant head at the top, in participant order.
    pub heads: Vec<(f32, f32)>,
    pub messages: Vec<MsgInfo>,
}

struct Out {
    scene: Scene,
    w: f32,
    h: f32,
    reqs: Reqs,
    info: SeqInfo,
}

fn head_size(meas: &Measured, p: &Participant) -> (f32, f32) {
    let (tw, th) = meas.size(p.label);
    match p.kind {
        PKind::Participant => ((tw + HEAD_PAD_X).max(24.0), th + HEAD_PAD_Y),
        PKind::Collections => ((tw + HEAD_PAD_X + 4.0).max(28.0), th + HEAD_PAD_Y + 4.0),
        PKind::Queue => ((tw + HEAD_PAD_X + 16.0).max(40.0), th + HEAD_PAD_Y),
        k => {
            let (iw, ih) = icon_size(icon_of(k));
            ((tw + 6.0).max(iw + 6.0), ih + 3.0 + th)
        }
    }
}

fn base_reqs(m: &Model, heads: &[(f32, f32)], box_w: &[f32]) -> Reqs {
    let n = m.participants.len();
    let mut reqs = Reqs::new();
    let boxed = |i: usize| m.participants[i].box_idx;
    for i in 0..n {
        let edge_gap = if boxed(i).is_some() { 10.0 } else { 5.0 };
        if i == 0 {
            add_req(&mut reqs, 0, 1, heads[0].0 / 2.0 + edge_gap + 2.0);
        } else {
            let mut gap = 10.0;
            if boxed(i) != boxed(i - 1) {
                gap += 10.0;
            }
            add_req(&mut reqs, i, i + 1, (heads[i - 1].0 + heads[i].0) / 2.0 + gap);
        }
    }
    add_req(&mut reqs, n, n + 1, heads[n - 1].0 / 2.0 + if boxed(n - 1).is_some() { 12.0 } else { 7.0 });
    // Box titles have to fit above their participants.
    for (bi, bw) in box_w.iter().enumerate() {
        let members: Vec<usize> = (0..n).filter(|&i| boxed(i) == Some(bi)).collect();
        if let (Some(&a), Some(&b)) = (members.first(), members.last()) {
            let inner = (heads[a].0 + heads[b].0) / 2.0;
            if a < b {
                add_req(&mut reqs, a + 1, b + 1, bw + 20.0 - inner);
            } else {
                add_req(&mut reqs, a, a + 1, (bw + 20.0) / 2.0);
                add_req(&mut reqs, a + 1, a + 2, (bw + 20.0) / 2.0);
            }
        }
    }
    reqs
}

fn draw(m: &Model, skin: &Skin, meas: &Measured, xs: &[f32], reqs0: Reqs) -> Out {
    let n = m.participants.len();
    let st = Style_ {
        lifeline: skin.border(&["sequencelifeline", "lifeline"], skin::BORDER),
        arrow: skin.color(&["sequencearrowcolor", "arrowcolor"], skin::BORDER),
        arrow_w: skin.num(&["sequencearrowthickness", "arrowthickness"], 1.0),
        act_fill: skin.fill(&["sequenceactivation", "sequencelifeline", "lifeline"], skin::PARTICIPANT_FILL),
        act_border: skin.border(&["sequenceactivation", "sequencelifeline", "lifeline"], skin::BORDER),
        group_border: skin.border(&["sequencegroup", "sequence", "group"], skin::BLACK),
        group_tab_fill: skin.fill(&["sequencegroupheader", "sequencegroup"], skin::GROUP_TAB_FILL),
        note_fill: skin.fill(&["note", "sequencenote"], skin::NOTE_FILL),
        note_border: skin.border(&["note", "sequencenote"], skin::BORDER),
        align: match skin.raw("sequencemessagealign").map(|s| s.to_ascii_lowercase()).as_deref() {
            Some("center") => HAlign::Center,
            Some("right") => HAlign::Right,
            _ => HAlign::Left,
        },
        below: skin.flag("responsemessagebelowarrow").unwrap_or(false),
    };

    let mut created = vec![false; n];
    for e in &m.events {
        if let Event::Message(msg) = e {
            if msg.creates {
                if let Endpoint::Part(p) = msg.to {
                    created[p] = true;
                }
            }
        }
    }

    let mut d = D {
        m,
        skin,
        meas,
        xs,
        st,
        reqs: reqs0,
        layers: Layers::default(),
        n,
        head: Vec::new(),
        created,
        cursor: 0.0,
        levels: (0..n).map(|_| Vec::new()).collect(),
        life_start: vec![0.0; n],
        life_end: vec![None; n],
        delays: Vec::new(),
        groups: Vec::new(),
        min_x: xs[0],
        max_x: xs[n + 1],
        last_msg: None,
        messages: Vec::new(),
        top_heads: vec![(0.0, 0.0); n],
        created_heads: Vec::new(),
        act_levels: Vec::new(),
    };
    d.head = m.participants.iter().map(|p| head_size(meas, p)).collect();

    // Box titles take a row above the participants.
    let box_title_h = m.boxes.iter().filter_map(|b| b.label).map(|l| meas.h(l)).fold(0.0, f32::max);
    let head_top = if m.boxes.iter().any(|b| b.label.is_some()) { box_title_h + 10.0 } else if m.boxes.is_empty() { 1.0 } else { 6.0 };
    let head_h = (0..n).filter(|&i| !d.created[i]).map(|i| d.head[i].1).fold(0.0, f32::max);
    let head_bottom = head_top + head_h;
    for i in 0..n {
        d.life_start[i] = head_bottom;
    }
    d.cursor = head_bottom + 6.0;
    d.run();

    // Unclosed activations run to the end.
    let line_end = d.cursor + 6.0;
    for p in 0..n {
        while d.level(p) > 0 {
            d.close_bar(p, line_end - 4.0);
        }
    }

    // Heads at the top and, mirrored, at the bottom.
    let foot_h = if m.hide_footbox { 0.0 } else { (0..n).filter(|&i| d.life_end[i].is_none()).map(|i| d.head[i].1).fold(0.0, f32::max) };
    for i in 0..n {
        let x = d.x(i);
        if !d.created[i] {
            let top = head_bottom - d.head[i].1;
            d.draw_head(i, x, top);
            d.top_heads[i] = (x - d.head[i].0 / 2.0, x + d.head[i].0 / 2.0);
        }
        if !m.hide_footbox && d.life_end[i].is_none() {
            d.draw_head(i, x, line_end);
        }
    }
    for (p, top) in d.created_heads.clone() {
        let x = d.x(p);
        d.draw_head(p, x, top);
        d.top_heads[p] = (x - d.head[p].0 / 2.0, x + d.head[p].0 / 2.0);
    }

    // Lifelines, dotted where a delay interrupts them.
    for i in 0..n {
        let y0 = d.life_start[i];
        let y1 = d.life_end[i].unwrap_or(line_end);
        let x = d.x(i);
        let mut cut = y0;
        let lw = skin.style.lifeline_width;
        let dash = match &skin.style.lifeline_dash {
            Some(p) => dashed(&d.st.lifeline, lw, p),
            None => solid(&d.st.lifeline, lw),
        };
        let dot = dashed(&d.st.lifeline, lw, &DOTTED);
        for &(a, b) in &d.delays {
            if b <= y0 || a >= y1 {
                continue;
            }
            if a > cut {
                d.layers.life.push_polyline(&[(x, cut), (x, a)], dash.clone());
            }
            d.layers.life.push_polyline(&[(x, a.max(y0)), (x, b.min(y1))], dot.clone());
            cut = b;
        }
        if y1 > cut {
            d.layers.life.push_polyline(&[(x, cut), (x, y1)], dash.clone());
        }
    }

    let bottom = line_end + foot_h + 2.0;

    // Participant boxes (`box ... end box`).
    for (bi, b) in m.boxes.iter().enumerate() {
        let members: Vec<usize> = (0..n).filter(|&i| m.participants[i].box_idx == Some(bi)).collect();
        let (Some(&a), Some(&z)) = (members.first(), members.last()) else { continue };
        let left = d.x(a) - d.head[a].0 / 2.0 - 5.0;
        let right = d.x(z) + d.head[z].0 / 2.0 + 5.0;
        let fill = b.fill.clone().unwrap_or_else(|| Paint::solid("#ffffff00"));
        d.layers.bg.push_rect(left, 0.0, right - left, bottom + 2.0, 0.0, Style::new().fill(fill).stroke(solid(&skin.style.box_border, skin.style.box_width)));
        if let Some(l) = b.label {
            let (cx, _) = ((left + right) / 2.0, 0.0);
            meas.text_at(&mut d.layers.bg, l, cx, 4.0, HAlign::Center);
        }
        d.bounds(left, right);
    }

    // Merge layers and move everything so the leftmost thing sits at x = 0.
    let (min_x, max_x) = (d.min_x, d.max_x);
    for lvl in std::mem::take(&mut d.act_levels) {
        d.layers.act.items.extend(lvl.items);
    }
    let mut scene = Scene::new(0.0, 0.0);
    for layer in [&mut d.layers.bg, &mut d.layers.hl, &mut d.layers.life, &mut d.layers.frame, &mut d.layers.act, &mut d.layers.top] {
        scene.items.append(&mut layer.items);
    }
    scene.translate(-min_x, 0.0);
    let shift = |x: f32| x - min_x;
    let info = SeqInfo {
        heads: d.top_heads.iter().map(|&(a, b)| (shift(a), shift(b))).collect(),
        messages: d
            .messages
            .iter()
            .map(|m| MsgInfo { y: m.y, from_x: shift(m.from_x), to_x: shift(m.to_x), label: m.label.map(|(a, b)| (shift(a), shift(b))), is_self: m.is_self })
            .collect(),
    };
    Out { scene, w: max_x - min_x, h: bottom, reqs: d.reqs, info }
}

pub fn layout(m: &Model, skin: &Skin, meas: &Measured) -> Body {
    layout_info(m, skin, meas).0
}

pub fn layout_info(m: &Model, skin: &Skin, meas: &Measured) -> (Body, SeqInfo) {
    let n = m.participants.len();
    let heads: Vec<(f32, f32)> = m.participants.iter().map(|p| head_size(meas, p)).collect();
    let box_w: Vec<f32> = m.boxes.iter().map(|b| b.label.map_or(0.0, |l| meas.w(l))).collect();
    let reqs = base_reqs(m, &heads, &box_w);
    let mut xs = solve(n + 2, &reqs);
    let mut out = draw(m, skin, meas, &xs, reqs);
    for _ in 0..8 {
        // draw only ever adds to the requirements it was given, so its output already
        // includes everything from the previous pass.
        let reqs = std::mem::take(&mut out.reqs);
        let next = solve(n + 2, &reqs);
        if next.iter().zip(&xs).all(|(a, b)| (a - b).abs() < 0.01) {
            break;
        }
        xs = next;
        out = draw(m, skin, meas, &xs, reqs);
    }
    let (w, h) = (out.w, out.h);
    (Body::new(out.scene, w, h), out.info)
}
