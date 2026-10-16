//! Activity diagram layout: top-down and structured, like PlantUML's.
//!
//! Every item is turned into a `Frag`, a piece of drawing with an entry point on top and
//! (unless the flow ends there) an exit point at the bottom. Fragments are stacked by
//! lining the exit of one up with the entry of the next; constructs with branches place
//! their branch fragments side by side and connect them with arrows.
//!
//! Swimlanes are handled last. While fragments are built, everything lives in one
//! lane-less coordinate system and each primitive just remembers its lane. At the end
//! every lane gets its own column (as wide as the widest thing in it) and the primitives
//! of a lane are shifted into it. Arrows between items are therefore stored as routes
//! (down, horizontal-then-vertical, ...) between two anchor points and only turned into
//! polylines once the final positions, which may be in different lanes, are known.

use super::shapes::{self, anchor_left, arrow, dashed, solid, Head, ARROW_DASH};
use super::Body;
use crate::plantuml::diagrams::activity::*;
use crate::plantuml::model::{LabelId, Measured};
use crate::plantuml::skin::{self, Skin};
use crate::scene::{HAlign, Item as SceneItem, Paint, Scene, Style, VAlign};

const GAP: f32 = 20.0;
const LANE_PAD: f32 = 10.0;

type Pt = (f32, f32);

#[derive(Clone, Default)]
struct Spec {
    color: Option<String>,
    dashed: bool,
    bold: bool,
    hidden: bool,
}

#[derive(Clone)]
enum Route {
    /// Straight down, or down-across-down when the ends are in different columns.
    Down,
    /// Horizontal first, then down (out of a diamond's side).
    Hv,
    /// Down first, then horizontal (into a merge diamond's side).
    Vh,
    /// Explicit points, shifted with the `from` lane.
    Raw(Vec<Pt>),
}

#[derive(Clone)]
struct Arr {
    route: Route,
    from: Pt,
    to: Pt,
    lane_from: usize,
    lane_to: usize,
    spec: Spec,
    head: bool,
}

#[derive(Clone)]
struct Text {
    id: LabelId,
    x: f32,
    y: f32,
    anchor: HAlign,
}

#[derive(Clone)]
enum Prim {
    Item(SceneItem, usize),
    /// Frames and other backgrounds, drawn under everything else.
    Back(SceneItem, usize),
    Arrow(Arr),
    Text(Text, usize),
}

#[derive(Clone)]
struct Frag {
    prims: Vec<Prim>,
    x0: f32,
    x1: f32,
    /// Bottom of everything in the fragment, in its own coordinates (top is 0).
    h: f32,
    entry: Pt,
    exit: Option<Pt>,
    lane_in: usize,
    lane_out: usize,
    breaks: Vec<(Pt, usize)>,
    in_arrow: Option<ArrowSpec>,
    /// Whether the arrow into this fragment has a head.
    head_in: bool,
    empty: bool,
    /// Boxes of the nodes (actions, diamonds, bars, ...) with their lane, for tests and
    /// for checking that arrows meet node borders.
    tiles: Vec<([f32; 4], usize)>,
}

impl Frag {
    fn new(lane: usize) -> Frag {
        Frag {
            prims: Vec::new(),
            x0: 0.0,
            x1: 0.0,
            h: 0.0,
            entry: (0.0, 0.0),
            exit: None,
            lane_in: lane,
            lane_out: lane,
            breaks: Vec::new(),
            in_arrow: None,
            head_in: true,
            empty: false,
            tiles: Vec::new(),
        }
    }

    fn empty() -> Frag {
        Frag { empty: true, exit: Some((0.0, 0.0)), ..Frag::new(0) }
    }

    fn shift(&mut self, dx: f32, dy: f32) {
        for p in &mut self.prims {
            match p {
                Prim::Item(it, _) | Prim::Back(it, _) => it.translate(dx, dy),
                Prim::Arrow(a) => {
                    a.from = (a.from.0 + dx, a.from.1 + dy);
                    a.to = (a.to.0 + dx, a.to.1 + dy);
                    if let Route::Raw(pts) = &mut a.route {
                        for q in pts {
                            *q = (q.0 + dx, q.1 + dy);
                        }
                    }
                }
                Prim::Text(t, _) => {
                    t.x += dx;
                    t.y += dy;
                }
            }
        }
        self.x0 += dx;
        self.x1 += dx;
        self.h += dy;
        self.entry = (self.entry.0 + dx, self.entry.1 + dy);
        self.exit = self.exit.map(|e| (e.0 + dx, e.1 + dy));
        for b in &mut self.breaks {
            b.0 = (b.0 .0 + dx, b.0 .1 + dy);
        }
        for t in &mut self.tiles {
            t.0 = [t.0[0] + dx, t.0[1] + dy, t.0[2] + dx, t.0[3] + dy];
        }
    }

    /// Takes over the drawing and extents of `other` (already in this frame).
    fn take(&mut self, other: Frag) {
        if self.prims.is_empty() && self.x0 == 0.0 && self.x1 == 0.0 {
            self.x0 = other.x0;
            self.x1 = other.x1;
        } else if !other.empty {
            self.x0 = self.x0.min(other.x0);
            self.x1 = self.x1.max(other.x1);
        }
        self.h = self.h.max(other.h);
        self.prims.extend(other.prims);
        self.breaks.extend(other.breaks);
        self.tiles.extend(other.tiles);
    }

    fn widen(&mut self, x0: f32, x1: f32) {
        self.x0 = self.x0.min(x0);
        self.x1 = self.x1.max(x1);
    }
}

struct Exit {
    x: f32,
    y: f32,
    lane: usize,
}

struct F<'a> {
    meas: &'a Measured,
    fill: Paint,
    border: String,
    diamond_fill: Paint,
    diamond_border: String,
    start_color: String,
    bar_color: String,
    arrow_color: String,
    note_fill: Paint,
    note_border: String,
    radius: f32,
    group_fill: Option<Paint>,
    group_border: String,
    lane_border: String,
}

fn spec_of(a: &Option<ArrowSpec>) -> Spec {
    a.as_ref().map(|a| Spec { color: a.color.clone(), dashed: a.dashed, bold: a.bold, hidden: a.hidden }).unwrap_or_default()
}

impl F<'_> {
    fn items(&self, draw: impl FnOnce(&mut Scene)) -> Vec<SceneItem> {
        let mut s = Scene::new(0.0, 0.0);
        draw(&mut s);
        s.items
    }

    fn put(&self, f: &mut Frag, lane: usize, items: Vec<SceneItem>) {
        f.prims.extend(items.into_iter().map(|i| Prim::Item(i, lane)));
    }

    fn text(&self, f: &mut Frag, lane: usize, id: LabelId, x: f32, y: f32, anchor: HAlign) {
        f.prims.push(Prim::Text(Text { id, x, y, anchor }, lane));
    }

    fn label_h(&self, id: Option<LabelId>) -> f32 {
        id.map_or(0.0, |i| self.meas.h(i))
    }

    // ---- arrows ----------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    fn arrow(&self, f: &mut Frag, route: Route, from: Pt, to: Pt, lanes: (usize, usize), spec: &Spec, head: bool) {
        f.prims.push(Prim::Arrow(Arr { route, from, to, lane_from: lanes.0, lane_to: lanes.1, spec: spec.clone(), head }));
    }

    fn plain(&self) -> Spec {
        Spec::default()
    }

    /// A downward arrow with the label of `spec` next to its middle.
    fn down(&self, f: &mut Frag, from: Pt, to: Pt, lanes: (usize, usize), spec: &Option<ArrowSpec>, head: bool) {
        self.arrow(f, Route::Down, from, to, lanes, &spec_of(spec), head);
        if let Some(l) = spec.as_ref().and_then(|s| s.label) {
            let mid = (from.1 + to.1) / 2.0;
            self.text(f, lanes.0, l, from.0 + 6.0, mid - self.meas.h(l) / 2.0, HAlign::Left);
        }
    }

    // ---- shapes ------------------------------------------------------------------

    fn diamond_size(&self, label: Option<LabelId>) -> (f32, f32) {
        let (tw, th) = label.map_or((0.0, 0.0), |l| self.meas.size(l));
        if tw == 0.0 { (24.0, 24.0) } else { ((tw + 40.0).max(24.0), (th + 12.0).max(24.0)) }
    }

    fn diamond(&self, f: &mut Frag, lane: usize, x: f32, y: f32, w: f32, h: f32, label: Option<LabelId>) {
        let tip = 12.0_f32.min(w / 2.0);
        let style = Style::new().fill(self.diamond_fill.clone()).stroke(solid(&self.diamond_border, 1.0));
        let items = self.items(|s| {
            s.push_polygon(&[(x, y + h / 2.0), (x + tip, y), (x + w - tip, y), (x + w, y + h / 2.0), (x + w - tip, y + h), (x + tip, y + h)], style);
            if let Some(l) = label {
                self.meas.text_in(s, l, x + tip, y, w - 2.0 * tip, h, HAlign::Center, VAlign::Middle);
            }
        });
        self.put(f, lane, items);
        f.tiles.push(([x, y, x + w, y + h], lane));
    }

    /// Small empty diamond where branches join.
    fn merge_diamond(&self, f: &mut Frag, lane: usize, cx: f32, y: f32) {
        self.diamond(f, lane, cx - 12.0, y, 24.0, 24.0, None);
    }

    fn pad_notes(&self, mut f: Frag, notes: &[Note], lane: usize) -> Frag {
        let mut ny = (f.h / 2.0).min(f.h);
        let mut left_y = None::<f32>;
        let mut right_y = None::<f32>;
        let (bx0, bx1) = (f.x0, f.x1);
        for n in notes {
            let (tw, th) = self.meas.size(n.label);
            let (w, h) = (tw + 21.0, th + 10.0);
            let slot = if n.right { &mut right_y } else { &mut left_y };
            let y = slot.unwrap_or((ny - h / 2.0).max(0.0));
            let x = if n.right { bx1 + 14.0 } else { bx0 - 14.0 - w };
            let fill = n.fill.clone().unwrap_or_else(|| self.note_fill.clone());
            let items = self.items(|s| {
                shapes::note_shape(s, shapes::NoteKind::Note, x, y, w, h, fill, &self.note_border, 1.0);
                self.meas.text_at(s, n.label, x + 7.0, y + 5.0, HAlign::Left);
                let (a, b) = if n.right { (bx1, x) } else { (x + w, bx0) };
                s.push_polyline(&[(a, y + h / 2.0), (b, y + h / 2.0)], dashed(&self.note_border, 1.0, &[2.0, 2.0]));
            });
            self.put(&mut f, lane, items);
            f.widen(x, x + w);
            f.h = f.h.max(y + h);
            *slot = Some(y + h + 6.0);
            ny = f.h;
        }
        f
    }

    // ---- nodes ---------------------------------------------------------------------

    fn node(&self, item: &Item) -> Frag {
        let lane = item.lane;
        let mut f = match &item.node {
            Node::Start => self.circle_node(lane, 20.0, &|s, x, y, d| {
                s.push_ellipse(x, y, d, d, Style::new().fill(Paint::solid(&self.start_color)));
            }),
            Node::Stop | Node::End => {
                let mut f = self.circle_node(lane, 22.0, &|s, x, y, d| {
                    s.push_ellipse(x, y, d, d, Style::new().stroke(solid(&self.start_color, 1.5)));
                    let r = 6.5;
                    s.push_ellipse(x + d / 2.0 - r, y + d / 2.0 - r, 2.0 * r, 2.0 * r, Style::new().fill(Paint::solid(&self.start_color)));
                });
                f.exit = None;
                f
            }
            Node::Kill => {
                let mut f = self.circle_node(lane, 20.0, &|s, x, y, d| {
                    let st = solid(&self.start_color, 2.0);
                    s.push_polyline(&[(x, y), (x + d, y + d)], st.clone());
                    s.push_polyline(&[(x, y + d), (x + d, y)], st);
                });
                f.exit = None;
                f
            }
            Node::Detach => {
                let mut f = Frag::new(lane);
                f.head_in = false;
                f
            }
            Node::Break => {
                let mut f = Frag::new(lane);
                f.head_in = false;
                f.breaks.push(((0.0, 0.0), lane));
                f
            }
            Node::Action { label, shape, fill } => self.action(lane, *label, *shape, fill.clone()),
            Node::Connector { label } => {
                let (tw, th) = self.meas.size(*label);
                let d = (tw + 12.0).max(th + 8.0).max(24.0);
                let items = self.items(|s| {
                    s.push_ellipse(0.0, 0.0, d, d, Style::new().fill(self.fill.clone()).stroke(solid(&self.border, 1.0)));
                    self.meas.text_in(s, *label, 0.0, 0.0, d, d, HAlign::Center, VAlign::Middle);
                });
                let mut f = Frag::new(lane);
                self.put(&mut f, lane, items);
                f.tiles.push(([0.0, 0.0, d, d], lane));
                f.x1 = d;
                f.h = d;
                f.entry = (d / 2.0, 0.0);
                f.exit = Some((d / 2.0, d));
                f
            }
            Node::If { branches, else_, end_lane } => self.if_node(lane, *end_lane, branches, else_),
            Node::Switch { cond, cases, end_lane } => self.switch(lane, *end_lane, *cond, cases),
            Node::While { cond, is_label, end_label, body, end_lane } => self.while_(lane, *end_lane, *cond, *is_label, *end_label, body),
            Node::Repeat { body, cond, is_label, not_label, backward, end_lane } => self.repeat(lane, *end_lane, body, *cond, *is_label, *not_label, backward.as_deref()),
            Node::Fork { branches, merge, end_lane } => self.fork(lane, *end_lane, branches, *merge),
            Node::Split { branches, end_lane } => self.split(lane, *end_lane, branches),
            Node::Group { kind, title, fill, body } => self.group(lane, *kind, *title, fill.clone(), body),
        };
        if !item.notes.is_empty() {
            f = self.pad_notes(f, &item.notes, lane);
        }
        f.in_arrow = item.arrow.clone();
        f
    }

    fn circle_node(&self, lane: usize, d: f32, draw: &dyn Fn(&mut Scene, f32, f32, f32)) -> Frag {
        let mut f = Frag::new(lane);
        let items = self.items(|s| draw(s, 0.0, 0.0, d));
        self.put(&mut f, lane, items);
        f.tiles.push(([0.0, 0.0, d, d], lane));
        f.x1 = d;
        f.h = d;
        f.entry = (d / 2.0, 0.0);
        f.exit = Some((d / 2.0, d));
        f
    }

    fn action(&self, lane: usize, label: LabelId, shape: ActionShape, fill: Option<Paint>) -> Frag {
        let (tw, th) = self.meas.size(label);
        let extra = if shape == ActionShape::Plain { 0.0 } else { 12.0 };
        let (w, h) = ((tw + 24.0 + extra).max(30.0), (th + 18.0).max(24.0));
        let r = self.radius.min(h / 2.0);
        let style = Style::new().fill(fill.unwrap_or_else(|| self.fill.clone())).stroke(solid(&self.border, 1.0));
        let items = self.items(|s| {
            let tip = 10.0;
            match shape {
                ActionShape::Plain => s.push_rect(0.0, 0.0, w, h, r, style),
                ActionShape::Procedure => {
                    s.push_rect(0.0, 0.0, w, h, 0.0, style);
                    s.push_polyline(&[(6.0, 0.0), (6.0, h)], solid(&self.border, 1.0));
                    s.push_polyline(&[(w - 6.0, 0.0), (w - 6.0, h)], solid(&self.border, 1.0));
                }
                ActionShape::Input => s.push_polygon(&[(0.0, h / 2.0), (tip, 0.0), (w, 0.0), (w, h), (tip, h)], style),
                ActionShape::Output => s.push_polygon(&[(0.0, 0.0), (w - tip, 0.0), (w, h / 2.0), (w - tip, h), (0.0, h)], style),
                ActionShape::Slant => s.push_polygon(&[(0.0, 0.0), (w, 0.0), (w - tip, h), (0.0, h)], style),
                ActionShape::Concave => s.push_polygon(&[(0.0, 0.0), (w, 0.0), (w - 6.0, h / 2.0), (w, h), (0.0, h)], style),
                ActionShape::Convex => s.push_polygon(&[(0.0, 0.0), (w - 6.0, 0.0), (w, h / 2.0), (w - 6.0, h), (0.0, h)], style),
            }
            let (tx, tw2) = match shape {
                ActionShape::Input => (tip / 2.0, w - tip / 2.0),
                ActionShape::Output | ActionShape::Convex => (0.0, w - tip / 2.0),
                _ => (0.0, w),
            };
            self.meas.text_in(s, label, tx, 0.0, tw2, h, HAlign::Center, VAlign::Middle);
        });
        let mut f = Frag::new(lane);
        self.put(&mut f, lane, items);
        f.tiles.push(([0.0, 0.0, w, h], lane));
        f.x1 = w;
        f.h = h;
        f.entry = (w / 2.0, 0.0);
        f.exit = Some((w / 2.0, h));
        f
    }

    // ---- stacking ---------------------------------------------------------------------

    fn block(&self, items: &[Item]) -> Frag {
        if items.is_empty() {
            return Frag::empty();
        }
        self.stack(items.iter().map(|i| self.node(i)).collect())
    }

    fn stack(&self, frags: Vec<Frag>) -> Frag {
        let mut out: Option<Frag> = None;
        for mut f in frags {
            let Some(o) = out.as_mut() else {
                out = Some(f);
                continue;
            };
            let connect = o.exit.is_some();
            let (px, py) = o.exit.unwrap_or((o.entry.0, o.h));
            let label_h = self.label_h(f.in_arrow.as_ref().and_then(|a| a.label));
            let gap = if !connect {
                10.0
            } else if !f.head_in {
                12.0
            } else {
                GAP.max(label_h + 8.0)
            };
            let dx = if connect { px - f.entry.0 } else { o.entry.0 - f.entry.0 };
            let dy = (py + gap - f.entry.1).max(o.h + 10.0);
            f.shift(dx, dy);
            if connect {
                self.down(o, (px, py), f.entry, (o.lane_out, f.lane_in), &f.in_arrow, f.head_in);
            }
            o.exit = f.exit;
            o.lane_out = f.lane_out;
            o.take(f);
        }
        out.unwrap_or_else(Frag::empty)
    }

    /// Places fragments side by side with `gap` between their boxes; returns the x
    /// offset of each.
    fn row<'a>(&self, frags: impl IntoIterator<Item = &'a Frag>, gap: f32) -> Vec<f32> {
        let mut dxs = Vec::new();
        let mut right = f32::NEG_INFINITY;
        for f in frags {
            let w = (f.x1 - f.x0).max(0.0);
            let dx = if right == f32::NEG_INFINITY { 0.0 } else { right + gap - f.x0 };
            dxs.push(dx);
            right = f.x0 + dx + w.max(if f.empty { 20.0 } else { 0.0 });
        }
        dxs
    }

    /// Connects `exits` into a merge diamond centered at `mx` whose top is at `y`.
    fn join_into_merge(&self, f: &mut Frag, exits: &[Exit], mx: f32, y: f32, lane: usize) {
        self.merge_diamond(f, lane, mx, y);
        for e in exits {
            let (route, to) = if (e.x - mx).abs() < 1.0 {
                (Route::Down, (mx, y))
            } else if e.x < mx {
                (Route::Vh, (mx - 12.0, y + 12.0))
            } else {
                (Route::Vh, (mx + 12.0, y + 12.0))
            };
            self.arrow(f, route, (e.x, e.y), to, (e.lane, lane), &self.plain(), true);
        }
        f.exit = Some((mx, y + 24.0));
        f.lane_out = lane;
        f.h = f.h.max(y + 24.0);
    }

    fn exit_of(&self, child: &Frag, dx: f32, dy: f32) -> Option<Exit> {
        child.exit.map(|e| Exit { x: e.0 + dx, y: e.1 + dy, lane: child.lane_out })
    }

    // ---- if / switch ----------------------------------------------------------------------

    fn if_node(&self, lane: usize, end_lane: usize, branches: &[IfBranch], else_: &Option<(Option<LabelId>, Block)>) -> Frag {
        let (mut f, exits) = self.if_chain(lane, branches, else_);
        if exits.is_empty() {
            f.exit = None;
            return f;
        }
        let y = (exits.iter().map(|e| e.y).fold(0.0, f32::max) + GAP).max(f.h + 10.0);
        let mx = f.entry.0;
        self.join_into_merge(&mut f, &exits, mx, y, end_lane);
        f
    }

    /// The condition diamond with its first branch to the left and everything else
    /// (further `elseif`s or the `else`) to the right. Returns the unjoined exits.
    fn if_chain(&self, lane: usize, branches: &[IfBranch], else_: &Option<(Option<LabelId>, Block)>) -> (Frag, Vec<Exit>) {
        let b0 = &branches[0];
        let cond = Some(b0.cond).filter(|c| !self.meas.label(*c).runs.is_empty());
        let (dw, dh) = self.diamond_size(cond);
        let mut left = self.block(&b0.body);
        let (mut right, mut right_exits, right_label, right_is_empty) = if branches.len() > 1 {
            let (f, ex) = self.if_chain(lane, &branches[1..], else_);
            (f, Some(ex), None, false)
        } else if let Some((lbl, body)) = else_ {
            let f = self.block(body);
            let e = f.empty;
            (f, None, *lbl, e)
        } else {
            (Frag::empty(), None, None, true)
        };
        // With branches in different swimlanes, keep the one in the lane further left on
        // the left so the arrows do not cross the diamond.
        let mut b0_label = b0.label;
        let mut right_label = right_label;
        if branches.len() == 1 && !left.empty && !right.empty && left.lane_in > right.lane_in {
            std::mem::swap(&mut left, &mut right);
            std::mem::swap(&mut b0_label, &mut right_label);
        }
        let lw0 = b0_label.map_or(0.0, |l| self.meas.w(l));
        let lw1 = right_label.map_or(0.0, |l| self.meas.w(l));
        let (run0, run1) = ((lw0 + 10.0).max(16.0), (lw1 + 10.0).max(16.0));

        let in0 = left.in_arrow.clone();
        let in1 = right.in_arrow.clone();
        let (e0, e1) = (left.entry.0, right.entry.0);
        let gap = 28.0;
        let dxs = self.row([&left, &right], gap);
        let mut dx_r = dxs[1];
        dx_r = dx_r.max(e0 + dw + run0 + run1 - e1);
        let x_e1 = e1 + dx_r;
        let total = x_e1 - e0;
        let cx = e0 + run0 + dw / 2.0 + (total - dw - run0 - run1) / 2.0;
        let (tip_l, tip_r) = ((cx - dw / 2.0, dh / 2.0), (cx + dw / 2.0, dh / 2.0));
        let y_child = dh / 2.0 + GAP;

        let mut f = Frag::new(lane);
        f.entry = (cx, 0.0);
        self.diamond(&mut f, lane, cx - dw / 2.0, 0.0, dw, dh, cond);
        f.x0 = cx - dw / 2.0;
        f.x1 = cx + dw / 2.0;
        f.h = dh;

        let mut exits = Vec::new();
        let sp0 = spec_of(&in0);
        let sp1 = spec_of(&in1);

        // Left branch.
        if left.empty {
            self.arrow(&mut f, Route::Raw(vec![tip_l, (e0, dh / 2.0)]), tip_l, (e0, dh / 2.0), (lane, lane), &sp0, false);
            exits.push(Exit { x: e0, y: dh / 2.0, lane });
        } else {
            left.shift(0.0, y_child);
            let lane_in = left.lane_in;
            self.arrow(&mut f, Route::Hv, tip_l, left.entry, (lane, lane_in), &sp0, true);
            exits.extend(self.exit_of(&left, 0.0, 0.0));
        }
        if let Some(l) = in0.as_ref().and_then(|a| a.label).or(b0_label) {
            self.text(&mut f, lane, l, tip_l.0 - 4.0, dh / 2.0 - self.meas.h(l) - 1.0, HAlign::Right);
        }
        f.take(left);

        // Right side.
        if right_is_empty && right_exits.is_none() {
            self.arrow(&mut f, Route::Raw(vec![tip_r, (x_e1, dh / 2.0)]), tip_r, (x_e1, dh / 2.0), (lane, lane), &sp1, false);
            exits.push(Exit { x: x_e1, y: dh / 2.0, lane });
        } else {
            right.shift(dx_r, y_child);
            let lane_in = right.lane_in;
            self.arrow(&mut f, Route::Hv, tip_r, right.entry, (lane, lane_in), &sp1, true);
            match right_exits.take() {
                Some(list) => exits.extend(list.into_iter().map(|e| Exit { x: e.x + dx_r, y: e.y + y_child, lane: e.lane })),
                None => exits.extend(self.exit_of(&right, 0.0, 0.0)),
            }
        }
        if let Some(l) = in1.as_ref().and_then(|a| a.label).or(right_label) {
            self.text(&mut f, lane, l, tip_r.0 + 4.0, dh / 2.0 - self.meas.h(l) - 1.0, HAlign::Left);
        }
        f.take(right);
        f.x0 = f.x0.min(e0.min(0.0));
        (f, exits)
    }

    fn switch(&self, lane: usize, end_lane: usize, cond: LabelId, cases: &[Case]) -> Frag {
        let cond_l = Some(cond).filter(|c| !self.meas.label(*c).runs.is_empty());
        let (dw, dh) = self.diamond_size(cond_l);
        let mut frags: Vec<Frag> = cases.iter().map(|c| self.block(&c.body)).collect();
        if frags.is_empty() {
            let mut f = Frag::new(lane);
            self.diamond(&mut f, lane, 0.0, 0.0, dw, dh, cond_l);
            f.x1 = dw;
            f.h = dh;
            f.entry = (dw / 2.0, 0.0);
            f.exit = Some((dw / 2.0, dh));
            return f;
        }
        let dxs = self.row(&frags, 28.0);
        let xs: Vec<f32> = frags.iter().zip(&dxs).map(|(f, dx)| f.entry.0 + dx).collect();
        let (xmin, xmax) = (xs.iter().cloned().fold(f32::MAX, f32::min), xs.iter().cloned().fold(f32::MIN, f32::max));
        let cx = (xmin + xmax) / 2.0;
        let bus_y = dh + 12.0;
        let y_child = bus_y + 24.0 + cases.iter().map(|c| self.label_h(c.label)).fold(0.0, f32::max);

        let mut f = Frag::new(lane);
        self.diamond(&mut f, lane, cx - dw / 2.0, 0.0, dw, dh, cond_l);
        f.x0 = cx - dw / 2.0;
        f.x1 = cx + dw / 2.0;
        f.entry = (cx, 0.0);
        f.h = dh;
        self.arrow(&mut f, Route::Raw(vec![(cx, dh), (cx, bus_y)]), (cx, dh), (cx, bus_y), (lane, lane), &self.plain(), false);
        if xs.len() > 1 {
            self.arrow(&mut f, Route::Raw(vec![(xmin, bus_y), (xmax, bus_y)]), (xmin, bus_y), (xmax, bus_y), (lane, lane), &self.plain(), false);
        }
        let mut exits = Vec::new();
        for (i, fr) in frags.iter_mut().enumerate() {
            let label = cases[i].label;
            let x = xs[i];
            if fr.empty {
                self.arrow(&mut f, Route::Raw(vec![(x, bus_y), (x, y_child)]), (x, bus_y), (x, y_child), (lane, lane), &self.plain(), false);
                exits.push(Exit { x, y: y_child, lane });
            } else {
                fr.shift(dxs[i], y_child);
                let spec = spec_of(&fr.in_arrow);
                self.arrow(&mut f, Route::Down, (x, bus_y), fr.entry, (lane, fr.lane_in), &spec, true);
                exits.extend(self.exit_of(fr, 0.0, 0.0));
            }
            if let Some(l) = label {
                self.text(&mut f, lane, l, x + 6.0, bus_y + 4.0, HAlign::Left);
            }
        }
        for fr in frags {
            f.take(fr);
        }
        if exits.is_empty() {
            return f;
        }
        let y = (exits.iter().map(|e| e.y).fold(0.0, f32::max) + GAP).max(f.h + 10.0);
        self.join_into_merge(&mut f, &exits, cx, y, end_lane);
        f
    }

    // ---- loops -----------------------------------------------------------------------------------

    fn while_(&self, lane: usize, end_lane: usize, cond: LabelId, is_label: Option<LabelId>, end_label: Option<LabelId>, body: &Block) -> Frag {
        let cond_l = Some(cond).filter(|c| !self.meas.label(*c).runs.is_empty());
        let (dw, dh) = self.diamond_size(cond_l);
        let mut b = self.block(body);
        let cx = if b.empty { dw / 2.0 } else { b.entry.0 };
        let mut f = Frag::new(lane);
        f.entry = (cx, 0.0);
        self.diamond(&mut f, lane, cx - dw / 2.0, 0.0, dw, dh, cond_l);
        f.x0 = cx - dw / 2.0;
        f.x1 = cx + dw / 2.0;
        f.h = dh;
        let tip_r = (cx + dw / 2.0, dh / 2.0);
        let tip_l = (cx - dw / 2.0, dh / 2.0);

        if b.empty {
            f.exit = Some((cx, dh));
            f.lane_out = end_lane;
            return f;
        }
        let label_h = self.label_h(is_label).max(0.0);
        let dy = dh + GAP.max(label_h + 8.0);
        b.shift(0.0, dy);
        let arrow_spec = b.in_arrow.clone().map(|mut a| {
            a.label = a.label.or(is_label);
            a
        });
        let arrow_spec = arrow_spec.or(Some(ArrowSpec { label: is_label, ..Default::default() }));
        self.down(&mut f, (cx, dh), b.entry, (lane, b.lane_in), &arrow_spec, true);

        let (bx0, bx1) = (b.x0.min(f.x0), b.x1.max(f.x1));
        let xl = bx0 - 14.0;
        let xr = bx1 + 14.0 + end_label.map_or(0.0, |l| (self.meas.w(l) + 8.0 - (f.x1 - cx)).max(0.0));
        let mut y_end = b.h + 14.0;
        if let Some((ex, ey)) = b.exit {
            let ly = ey + 14.0;
            self.arrow(&mut f, Route::Raw(vec![(ex, ey), (ex, ly), (xl, ly), (xl, dh / 2.0), tip_l]), (ex, ey), tip_l, (b.lane_out, lane), &self.plain(), true);
            y_end = ly + 14.0;
            f.widen(xl, xl);
        }
        for (bp, bl) in &b.breaks {
            self.arrow(&mut f, Route::Raw(vec![*bp, (xr, bp.1)]), *bp, (xr, bp.1), (*bl, *bl), &self.plain(), false);
            y_end = y_end.max(bp.1 + 14.0);
        }
        self.arrow(&mut f, Route::Raw(vec![tip_r, (xr, tip_r.1), (xr, y_end), (cx, y_end)]), tip_r, (cx, y_end), (lane, end_lane), &self.plain(), false);
        if let Some(l) = end_label {
            self.text(&mut f, lane, l, tip_r.0 + 4.0, tip_r.1 - self.meas.h(l) - 1.0, HAlign::Left);
        }
        f.take(b);
        f.widen(xl, xr);
        f.h = f.h.max(y_end);
        f.exit = Some((cx, y_end));
        f.lane_out = end_lane;
        f
    }

    #[allow(clippy::too_many_arguments)]
    fn repeat(&self, lane: usize, end_lane: usize, body: &Block, cond: Option<LabelId>, is_label: Option<LabelId>, not_label: Option<LabelId>, backward: Option<&Item>) -> Frag {
        let mut b = self.block(body);
        let cond_l = cond.filter(|c| !self.meas.label(*c).runs.is_empty());
        let (dw, dh) = self.diamond_size(cond_l);
        let mut f = Frag::new(lane);
        if b.empty {
            b = self.action_placeholder(lane);
        }
        let ex = b.entry.0;
        f.entry = (ex, 0.0);
        f.x0 = b.x0;
        f.x1 = b.x1;
        f.h = b.h;
        let Some((bx, by)) = b.exit else {
            f.exit = None;
            f.take(b);
            return f;
        };
        let y_d = by + GAP;
        let cx = bx;
        self.arrow(&mut f, Route::Down, (bx, by), (cx, y_d), (b.lane_out, lane), &self.plain(), true);
        self.diamond(&mut f, lane, cx - dw / 2.0, y_d, dw, dh, cond_l);
        let tip_r = (cx + dw / 2.0, y_d + dh / 2.0);
        f.widen(cx - dw / 2.0, cx + dw / 2.0);

        let mut xr = (b.x1.max(cx + dw / 2.0) + 16.0).max(cx + dw / 2.0 + is_label.map_or(0.0, |l| self.meas.w(l)) + 12.0);
        let mut back = None;
        if let Some(bk) = backward {
            let bf = self.node(bk);
            xr = xr.max(b.x1 + 16.0);
            back = Some(bf);
        }
        let top_y = -10.0;
        match back {
            Some(mut bf) => {
                let w = bf.x1 - bf.x0;
                let mid = (tip_r.1 + top_y) / 2.0;
                let h = bf.h;
                bf.shift(xr - bf.entry.0, mid - h / 2.0);
                let (box_top, box_bottom) = (mid - h / 2.0, mid + h / 2.0);
                self.arrow(&mut f, Route::Raw(vec![tip_r, (xr, tip_r.1), (xr, box_bottom)]), tip_r, (xr, box_bottom), (lane, lane), &self.plain(), false);
                self.arrow(&mut f, Route::Raw(vec![(xr, box_top), (xr, top_y), (ex, top_y)]), (xr, box_top), (ex, top_y), (lane, lane), &self.plain(), false);
                f.take(bf);
                f.widen(xr - w / 2.0, xr + w / 2.0);
            }
            None => {
                self.arrow(&mut f, Route::Raw(vec![tip_r, (xr, tip_r.1), (xr, top_y), (ex, top_y)]), tip_r, (ex, top_y), (lane, lane), &self.plain(), false);
                f.widen(xr, xr);
            }
        }
        if let Some(l) = is_label {
            self.text(&mut f, lane, l, tip_r.0 + 4.0, tip_r.1 - self.meas.h(l) - 1.0, HAlign::Left);
        }
        if let Some(l) = not_label {
            self.text(&mut f, lane, l, cx + 6.0, y_d + dh + 3.0, HAlign::Left);
        }
        for (bp, bl) in b.breaks.clone() {
            self.arrow(&mut f, Route::Raw(vec![bp, (xr, bp.1)]), bp, (xr, bp.1), (bl, bl), &self.plain(), false);
        }
        f.take(b);
        f.h = f.h.max(y_d + dh);
        f.exit = Some((cx, y_d + dh));
        f.lane_out = end_lane;
        f
    }

    /// Stand-in for an empty loop body so there is something to loop around.
    fn action_placeholder(&self, lane: usize) -> Frag {
        let mut f = Frag::new(lane);
        f.x1 = 0.0;
        f.entry = (0.0, 0.0);
        f.exit = Some((0.0, 0.0));
        f
    }

    // ---- fork / split ----------------------------------------------------------------------------

    fn fork(&self, lane: usize, end_lane: usize, branches: &[Block], merge: bool) -> Frag {
        let mut frags: Vec<Frag> = branches.iter().map(|b| self.block(b)).collect();
        let dxs = self.row(&frags, 28.0);
        let es: Vec<f32> = frags.iter().zip(&dxs).map(|(f, dx)| if f.empty { f.x0 + dx + 10.0 } else { f.entry.0 + dx }).collect();
        let (emin, emax) = (es.iter().cloned().fold(f32::MAX, f32::min), es.iter().cloned().fold(f32::MIN, f32::max));
        let center = (emin + emax) / 2.0;
        let (bar_x0, bar_x1) = ((emin - 12.0).min(center - 20.0), (emax + 12.0).max(center + 20.0));
        let bar_h = 6.0;
        let y_child = bar_h + GAP;

        let mut f = Frag::new(lane);
        f.entry = (center, 0.0);
        let bar = |me: &Self, f: &mut Frag, lane: usize, y: f32| {
            let items = me.items(|s| s.push_rect(bar_x0, y, bar_x1 - bar_x0, bar_h, 2.0, Style::new().fill(Paint::solid(&me.bar_color))));
            me.put(f, lane, items);
            f.tiles.push(([bar_x0, y, bar_x1, y + bar_h], lane));
        };
        bar(self, &mut f, lane, 0.0);
        f.x0 = bar_x0;
        f.x1 = bar_x1;
        f.h = bar_h;

        let mut exits = Vec::new();
        for (i, fr) in frags.iter_mut().enumerate() {
            let x = es[i];
            if fr.empty {
                exits.push(Exit { x, y: bar_h, lane });
                continue;
            }
            fr.shift(dxs[i], y_child);
            let spec = fr.in_arrow.clone();
            self.down(&mut f, (x, bar_h), fr.entry, (lane, fr.lane_in), &spec, true);
            exits.extend(self.exit_of(fr, 0.0, 0.0));
        }
        for fr in frags {
            f.take(fr);
        }
        if exits.is_empty() {
            return f;
        }
        let y = (exits.iter().map(|e| e.y).fold(0.0, f32::max) + GAP).max(f.h + 10.0);
        if merge {
            self.join_into_merge(&mut f, &exits, center, y, end_lane);
        } else {
            for e in &exits {
                self.arrow(&mut f, Route::Down, (e.x, e.y), (e.x, y), (e.lane, end_lane), &self.plain(), true);
            }
            bar(self, &mut f, end_lane, y);
            f.exit = Some((center, y + bar_h));
            f.lane_out = end_lane;
            f.h = y + bar_h;
        }
        f
    }

    fn split(&self, lane: usize, end_lane: usize, branches: &[Block]) -> Frag {
        let mut frags: Vec<Frag> = branches.iter().map(|b| self.block(b)).collect();
        let dxs = self.row(&frags, 28.0);
        let es: Vec<f32> = frags.iter().zip(&dxs).map(|(f, dx)| if f.empty { f.x0 + dx + 10.0 } else { f.entry.0 + dx }).collect();
        let (emin, emax) = (es.iter().cloned().fold(f32::MAX, f32::min), es.iter().cloned().fold(f32::MIN, f32::max));
        let center = (emin + emax) / 2.0;
        let mut f = Frag::new(lane);
        f.entry = (center, 0.0);
        f.x0 = emin;
        f.x1 = emax;
        self.arrow(&mut f, Route::Raw(vec![(emin, 0.0), (emax, 0.0)]), (emin, 0.0), (emax, 0.0), (lane, lane), &self.plain(), false);
        let mut exits = Vec::new();
        for (i, fr) in frags.iter_mut().enumerate() {
            let x = es[i];
            if fr.empty {
                exits.push(Exit { x, y: 0.0, lane });
                continue;
            }
            fr.shift(dxs[i], GAP);
            let spec = fr.in_arrow.clone();
            self.down(&mut f, (x, 0.0), fr.entry, (lane, fr.lane_in), &spec, true);
            exits.extend(self.exit_of(fr, 0.0, 0.0));
        }
        for fr in frags {
            f.take(fr);
        }
        if exits.is_empty() {
            return f;
        }
        let y = (exits.iter().map(|e| e.y).fold(0.0, f32::max) + GAP).max(f.h + 10.0);
        for e in &exits {
            self.arrow(&mut f, Route::Raw(vec![(e.x, e.y), (e.x, y)]), (e.x, e.y), (e.x, y), (e.lane, e.lane), &self.plain(), false);
        }
        self.arrow(&mut f, Route::Raw(vec![(emin, y), (emax, y)]), (emin, y), (emax, y), (end_lane, end_lane), &self.plain(), false);
        f.exit = Some((center, y));
        f.lane_out = end_lane;
        f.h = f.h.max(y);
        f
    }

    // ---- groups ------------------------------------------------------------------------------------

    fn group(&self, lane: usize, kind: GroupKind, title: Option<LabelId>, fill: Option<Paint>, body: &Block) -> Frag {
        let mut b = self.block(body);
        let (tw, th) = title.map_or((0.0, 0.0), |t| self.meas.size(t));
        let tab = if kind == GroupKind::Package { 8.0 } else { 0.0 };
        let top_h = if title.is_some() { th + 10.0 + tab } else { 10.0 };
        let (bx0, bx1) = if b.empty { (0.0, 0.0) } else { (b.x0, b.x1) };
        let pad = 12.0;
        let mut x0 = bx0 - pad;
        let mut x1 = bx1 + pad;
        if x1 - x0 < tw + 24.0 {
            let mid = if b.empty { 0.0 } else { b.entry.0 };
            x0 = x0.min(mid - (tw + 24.0) / 2.0);
            x1 = x1.max(mid + (tw + 24.0) / 2.0);
        }
        let dy = top_h + 6.0;
        b.shift(0.0, dy);
        let bottom = b.h + pad;
        let frame_fill = fill.or_else(|| self.group_fill.clone());
        let mut f = Frag::new(lane);
        let radius = 2.5;
        let items = self.items(|s| {
            let mut st = Style::new().stroke(solid(&self.group_border, 1.0));
            if let Some(ff) = frame_fill {
                st = st.fill(ff);
            }
            match kind {
                GroupKind::Package => {
                    let tw2 = (tw + 20.0).min(x1 - x0);
                    s.push_rect(x0, tab, x1 - x0, bottom - tab, 0.0, st);
                    s.push_polyline(&[(x0, tab), (x0, 0.0), (x0 + tw2, 0.0), (x0 + tw2, tab)], solid(&self.group_border, 1.0));
                }
                _ => s.push_rect(x0, 0.0, x1 - x0, bottom, radius, st),
            }
            if let Some(t) = title {
                let ty = if kind == GroupKind::Package { tab + 5.0 } else { 5.0 };
                self.meas.text_at(s, t, x0 + 10.0, ty, HAlign::Left);
            }
        });
        f.prims.extend(items.into_iter().map(|i| Prim::Back(i, lane)));
        f.x0 = x0;
        f.x1 = x1;
        f.h = bottom;
        f.entry = if b.empty { ((x0 + x1) / 2.0, 0.0) } else { b.entry };
        f.exit = if b.empty { Some(((x0 + x1) / 2.0, bottom)) } else { b.exit };
        f.lane_in = b.lane_in;
        f.lane_out = b.lane_out;
        // Arrows enter and leave through the frame, so they connect to what is inside.
        f.in_arrow = None;
        f.take(b);
        f
    }
}

// ---- turning fragments into a scene ------------------------------------------------------------------

struct Bounds {
    min_x: Vec<f32>,
    max_x: Vec<f32>,
    min_y: f32,
    max_y: f32,
}

impl Bounds {
    fn add(&mut self, lane: usize, x0: f32, x1: f32, y0: f32, y1: f32) {
        self.min_x[lane] = self.min_x[lane].min(x0);
        self.max_x[lane] = self.max_x[lane].max(x1);
        self.min_y = self.min_y.min(y0);
        self.max_y = self.max_y.max(y1);
    }
}

fn item_box(it: &SceneItem) -> (f32, f32, f32, f32) {
    match it {
        SceneItem::Shape(s) => (s.bbox[0], s.bbox[1], s.bbox[0] + s.bbox[2], s.bbox[1] + s.bbox[3]),
        SceneItem::Text(t) => (t.x, t.y, t.x + t.w, t.y + t.h),
    }
}

fn arrow_points(a: &Arr, dx_from: f32, dx_to: f32) -> Vec<Pt> {
    let (f, t) = ((a.from.0 + dx_from, a.from.1), (a.to.0 + dx_to, a.to.1));
    match &a.route {
        Route::Down => {
            if (f.0 - t.0).abs() < 0.5 {
                vec![f, (f.0, t.1)]
            } else {
                let mid = (f.1 + t.1) / 2.0;
                vec![f, (f.0, mid), (t.0, mid), t]
            }
        }
        Route::Hv => vec![f, (t.0, f.1), t],
        Route::Vh => vec![f, (f.0, t.1), t],
        Route::Raw(pts) => pts.iter().map(|p| (p.0 + dx_from, p.1)).collect(),
    }
}

/// Final geometry for the tests: node boxes and arrow polylines (with whether the arrow
/// has a head at its end).
#[derive(Debug, Clone, Default)]
pub struct FlowInfo {
    pub tiles: Vec<[f32; 4]>,
    pub arrows: Vec<(Vec<(f32, f32)>, bool)>,
}

pub fn layout(m: &Model, skin: &Skin, meas: &Measured) -> Body {
    layout_info(m, skin, meas).0
}

pub fn layout_info(m: &Model, skin: &Skin, meas: &Measured) -> (Body, FlowInfo) {
    let f = F {
        meas,
        fill: skin.fill(&["activity"], skin::ACTIVITY_FILL),
        border: skin.border(&["activity"], skin::BORDER),
        diamond_fill: skin.fill(&["activitydiamond", "activity"], skin::ACTIVITY_FILL),
        diamond_border: skin.border(&["activitydiamond", "activity"], skin::BORDER),
        start_color: skin.color(&["activitystartcolor", "activitybordercolor"], "#222222"),
        bar_color: skin.color(&["activitybarcolor"], "#222222"),
        arrow_color: skin.color(&["activityarrowcolor", "arrowcolor"], skin::BORDER),
        note_fill: skin.fill(&["note"], skin::NOTE_FILL),
        note_border: skin.border(&["note"], skin::BORDER),
        radius: skin.radius(&["activity"], 25.0),
        group_fill: skin.raw("partitionbackgroundcolor").and_then(crate::plantuml::colors::parse_paint),
        group_border: skin.border(&["partition", "package"], skin::BORDER),
        lane_border: skin.border(&["swimlane"], skin::BORDER),
    };
    let frag = f.block(&m.body);
    let lanes = m.lanes.len().max(1);
    // Lane indices past the last column are drawn in it.
    let lane_of = |l: usize| l.min(lanes - 1);

    let mut b = Bounds { min_x: vec![f32::MAX; lanes], max_x: vec![f32::MIN; lanes], min_y: 0.0, max_y: 0.0 };
    for p in &frag.prims {
        match p {
            Prim::Item(it, lane) | Prim::Back(it, lane) => {
                let (x0, y0, x1, y1) = item_box(it);
                b.add(lane_of(*lane), x0, x1, y0, y1);
            }
            Prim::Arrow(a) => {
                for q in arrow_points(a, 0.0, 0.0) {
                    b.add(lane_of(a.lane_from), q.0, q.0, q.1, q.1);
                }
                b.add(lane_of(a.lane_to), a.to.0, a.to.0, a.to.1, a.to.1);
            }
            Prim::Text(t, lane) => {
                let (w, h) = meas.size(t.id);
                let x0 = anchor_left(t.anchor, t.x, w);
                b.add(lane_of(*lane), x0, x0 + w, t.y, t.y + h);
            }
        }
    }
    b.max_y = b.max_y.max(frag.h);
    for l in 0..lanes {
        if b.min_x[l] > b.max_x[l] {
            b.min_x[l] = 0.0;
            b.max_x[l] = 40.0;
        }
    }

    // Columns.
    let swim = !m.lanes.is_empty();
    let pad = if swim { LANE_PAD } else { 0.0 };
    let mut widths = Vec::new();
    for l in 0..lanes {
        let mut w = b.max_x[l] - b.min_x[l] + 2.0 * pad;
        if swim {
            w = w.max(meas.w(m.lanes[l].label) + 20.0).max(80.0);
        }
        widths.push(w);
    }
    let mut origin = Vec::new();
    let mut acc = 0.0;
    for w in &widths {
        origin.push(acc);
        acc += w;
    }
    let total_w = acc;
    let dx: Vec<f32> = (0..lanes).map(|l| origin[l] + (widths[l] - (b.max_x[l] - b.min_x[l])) / 2.0 - b.min_x[l]).collect();
    let header_h = if swim { m.lanes.iter().map(|l| meas.h(l.label)).fold(0.0, f32::max) + 12.0 } else { 0.0 };
    let top = if swim { header_h + 10.0 } else { 0.0 } - b.min_y.min(0.0);
    let margin = 2.0;
    let total_h = top + b.max_y + if swim { 12.0 } else { margin };

    let mut scene = Scene::new(total_w, total_h);
    if swim {
        for (i, lane) in m.lanes.iter().enumerate() {
            let fill = lane.fill.clone();
            let mut st = Style::new().stroke(solid(&f.lane_border, 1.0));
            if let Some(fl) = fill {
                st = st.fill(fl);
            }
            scene.push_rect(origin[i], 0.0, widths[i], total_h, 0.0, st);
            scene.push_rect(origin[i], 0.0, widths[i], header_h, 0.0, Style::new().stroke(solid(&f.lane_border, 1.0)));
            meas.text_in(&mut scene, lane.label, origin[i], 0.0, widths[i], header_h, HAlign::Center, VAlign::Middle);
        }
    }

    let mut info = FlowInfo::default();
    for (t, lane) in &frag.tiles {
        let d = dx[lane_of(*lane)];
        info.tiles.push([t[0] + d, t[1] + top, t[2] + d, t[3] + top]);
    }
    let (backs, rest): (Vec<Prim>, Vec<Prim>) = frag.prims.into_iter().partition(|p| matches!(p, Prim::Back(..)));
    for p in backs.into_iter().chain(rest) {
        match p {
            Prim::Item(mut it, lane) | Prim::Back(mut it, lane) => {
                it.translate(dx[lane_of(lane)], top);
                scene.items.push(it);
            }
            Prim::Arrow(a) => {
                if a.spec.hidden {
                    continue;
                }
                let (lf, lt) = (lane_of(a.lane_from), lane_of(a.lane_to));
                let mut pts = arrow_points(&a, dx[lf], dx[lt]);
                for q in &mut pts {
                    q.1 += top;
                }
                let color = a.spec.color.clone().unwrap_or_else(|| f.arrow_color.clone());
                let w = if a.spec.bold { 2.0 } else { 1.0 };
                let stroke = if a.spec.dashed { dashed(&color, w, &ARROW_DASH) } else { solid(&color, w) };
                info.arrows.push((pts.clone(), a.head));
                arrow(&mut scene, &pts, Head::None, if a.head { Head::Flow } else { Head::None }, stroke);
            }
            Prim::Text(t, lane) => {
                meas.text_at(&mut scene, t.id, t.x + dx[lane_of(lane)], t.y + top, t.anchor);
            }
        }
    }
    (Body::new(scene, total_w, total_h), info)
}
