//! Drawing shared by the graph diagrams (class, state, use case, component): links with
//! their decorations, labels and cardinalities, containers, notes and a few outlines
//! (cloud, cylinder, 3D box) that more than one diagram type needs.

use super::graph::{dist, lerp, Edge, End, Graph, Hint, Layout, Opts, Pt, Rect, Route, Style as LineStyle};
use super::shapes::{arrowhead, dashed, head_inset, note_shape, solid, Head};
use crate::plantuml::diagrams::gcommon::{Arrow, Deco, GroupStyle, Link};
use crate::plantuml::model::{LabelId, Measured};
use crate::plantuml::skin::{self, Shapes, Skin};
use crate::scene::{HAlign, Paint, PathBuilder, Scene, Stroke, Style, VAlign};

/// What the tests check after a graph diagram is laid out: boxes and containers by
/// entity index, routes by link index.
#[derive(Default)]
pub struct GraphInfo {
    pub nodes: Vec<(usize, Rect)>,
    pub clusters: Vec<(usize, Rect)>,
    pub routes: Vec<(usize, Route)>,
    /// Labels that sit on links: link index, whether it is a multiplicity at an end, box.
    pub labels: Vec<(usize, bool, Rect)>,
    /// Container titles by entity index.
    pub titles: Vec<(usize, Rect)>,
    /// Per entity: its name as plain text (empty when it has none) and its container.
    pub names: Vec<String>,
    pub parents: Vec<Option<usize>>,
    /// Per link: the entities at its two ends, the text of its label and its source line.
    pub links: Vec<(usize, usize)>,
    pub link_labels: Vec<Option<String>>,
    pub link_lines: Vec<usize>,
}

impl GraphInfo {
    /// Fills in what the layout checks need to name things, runs them and attaches what
    /// they find to the scene.
    pub fn report(&mut self, scene: &mut Scene, names: Vec<String>, parents: Vec<Option<usize>>, links: &[Link], m: &Measured) {
        self.names = names;
        self.parents = parents;
        self.links = links.iter().map(|l| (l.from, l.to)).collect();
        self.link_labels = links.iter().map(|l| l.label.map(|id| m.plain(id))).collect();
        self.link_lines = links.iter().map(|l| l.line).collect();
        for w in super::lint::check(self) {
            scene.warn(w);
        }
    }
}

impl GraphInfo {
    /// Registers the boxes of a link's labels, from the sizes the layout was given.
    pub fn add_labels(&mut self, link: usize, route: &Route, label: Option<(f32, f32)>, tail: Option<(f32, f32)>, head: Option<(f32, f32)>) {
        let at = |c: Option<Pt>, s: Option<(f32, f32)>| c.zip(s).map(|(c, (w, h))| Rect::new(c.0 - w / 2.0, c.1 - h / 2.0, w, h));
        for (end, r) in [(false, at(route.label, label)), (true, at(route.tail_label, tail)), (true, at(route.head_label, head))] {
            if let Some(r) = r {
                self.labels.push((link, end, r));
            }
        }
    }
}

// ---- graph plumbing ------------------------------------------------------------------------

/// Where an entity of a graph diagram went in the graph handed to `graph::layout`.
pub enum Slot {
    Node(usize),
    Cluster(usize),
    /// A note that hangs on a link and is placed after routing.
    Floating,
}

impl Slot {
    pub fn cluster(&self) -> Option<usize> {
        match self {
            Slot::Cluster(c) => Some(*c),
            _ => None,
        }
    }

    fn end(&self) -> Option<End> {
        match self {
            Slot::Node(n) => Some(End::Node(*n)),
            Slot::Cluster(c) => Some(End::Cluster(*c)),
            Slot::Floating => None,
        }
    }
}

/// Dot options for a graph diagram, with `skinparam nodesep` and `ranksep` (pixels, as
/// in PlantUML) overriding the spacing.
pub fn graph_opts(skin: &Skin, lr: bool, reverse: bool) -> Opts {
    let d = Opts::default();
    Opts { lr, reverse, style: line_style(skin), node_sep: skin.num(&["nodesep"], d.node_sep).max(2.0), rank_sep: skin.num(&["ranksep"], d.rank_sep).max(2.0) }
}

pub fn line_style(skin: &Skin) -> LineStyle {
    match skin.raw("linetype").map(|v| v.to_ascii_lowercase()).as_deref() {
        Some("ortho") => LineStyle::Ortho,
        Some("polyline") => LineStyle::Polyline,
        _ => LineStyle::Spline,
    }
}

/// Adds an edge for every link whose ends are both in the graph and returns, per link,
/// the index of its edge. Only class diagrams have multiplicities (`cards`).
pub fn build_edges(g: &mut Graph, slots: &[Slot], links: &[Link], m: &Measured, cards: bool) -> Vec<Option<usize>> {
    links
        .iter()
        .map(|l| {
            let (from, to) = (slots[l.from].end()?, slots[l.to].end()?);
            let hint = l.arrow.dir.map_or(Hint::Auto(l.arrow.len), Hint::Dir);
            let label = l.label.map(|id| label_size(m, id, l.label_dir));
            let card = |c: Option<LabelId>| c.filter(|_| cards).map(|id| m.size(id));
            g.edges.push(Edge { from, to, hint, label, tail_label: card(l.from_card), head_label: card(l.to_card) });
            Some(g.edges.len() - 1)
        })
        .collect()
}

/// Draws the links. A link from or to a note (`is_note` of an entity) is a dashed
/// connector instead, drawn after all the others.
pub fn draw_links(scene: &mut Scene, lay: &Layout, links: &[Link], edge_of_link: &[Option<usize>], is_note: impl Fn(usize) -> bool, color: &str, cards: bool, skin: &Skin, m: &Measured) {
    let mut note_routes: Vec<&Route> = Vec::new();
    for (l, ei) in links.iter().zip(edge_of_link) {
        let Some(ei) = ei else { continue };
        if is_note(l.from) || is_note(l.to) {
            note_routes.push(&lay.routes[*ei]);
            continue;
        }
        let (from_card, to_card) = if cards { (l.from_card, l.to_card) } else { (None, None) };
        let d = LinkDraw { arrow: &l.arrow, color, label: l.label, label_dir: l.label_dir, from_card, to_card, shapes: &skin.style };
        draw_link(scene, &lay.routes[*ei], &d, m);
    }
    for r in note_routes {
        note_connector(scene, r, skin);
    }
}

/// Places a note that hangs on a link (`link` is its index) beside the link, clear of
/// the notes in `floating`, and draws it with a dashed line to the link.
pub fn draw_link_note(scene: &mut Scene, lay: &Layout, edge_of_link: &[Option<usize>], link: Option<usize>, id: LabelId, fill: Option<Paint>, floating: &mut Vec<Rect>, skin: &Skin, m: &Measured) {
    let Some(ei) = link.and_then(|l| edge_of_link.get(l).copied().flatten()) else { return };
    let route = &lay.routes[ei];
    let mid = route.label.unwrap_or_else(|| route.mid());
    let (w, h) = note_size(m, id);
    let r = lay.place_note(mid, w, h, floating);
    floating.push(r);
    draw_note(scene, m, id, &r, fill, skin);
    let border = skin.border(&["note"], skin::BORDER);
    scene.push_polyline(&note_link(mid, &r), dashed(&border, 1.0, &NOTE_DASH));
}

/// The boxes, containers, routes and label boxes of a laid-out graph diagram, by entity
/// and link index, for the layout checks and the tests.
pub fn collect_info(lay: &Layout, g: &Graph, slots: &[Slot], edge_of_link: &[Option<usize>], titles: Vec<(usize, Rect)>) -> GraphInfo {
    let mut info = GraphInfo::default();
    for (i, slot) in slots.iter().enumerate() {
        match slot {
            Slot::Node(n) => info.nodes.push((i, lay.nodes[*n])),
            Slot::Cluster(c) => info.clusters.push((i, lay.clusters[*c])),
            Slot::Floating => {}
        }
    }
    for (li, ei) in edge_of_link.iter().enumerate() {
        if let Some(ei) = ei {
            info.routes.push((li, lay.routes[*ei].clone()));
            let ge = &g.edges[*ei];
            info.add_labels(li, &lay.routes[*ei], ge.label, ge.tail_label, ge.head_label);
        }
    }
    info.titles = titles;
    info
}

pub const LINK_DASH: [f32; 2] = [7.0, 5.0];
/// Dash of the thin lines that tie notes to what they annotate.
const NOTE_DASH: [f32; 2] = [2.0, 2.0];
/// Width of the small triangle that marks a label's direction, plus its gap to the text.
pub const LABEL_ARROW: f32 = 12.0;

pub fn deco_head(d: Deco) -> Head {
    match d {
        Deco::None => Head::None,
        Deco::Arrow => Head::Filled,
        Deco::Triangle => Head::Triangle,
        Deco::Diamond => Head::Diamond,
        Deco::DiamondFilled => Head::DiamondFilled,
        Deco::Plus => Head::Plus,
        Deco::Cross => Head::Cross,
        Deco::Hash => Head::Square,
        Deco::Crowfoot => Head::Crowfoot,
        Deco::Lollipop => Head::Lollipop,
        Deco::Socket => Head::Socket,
        Deco::Circle => Head::Circle,
        Deco::Point => Head::Point,
    }
}

/// The outline of a box, dashed like links when its look asks for it.
pub fn outline(color: &str, width: f32, dashed_line: bool) -> Stroke {
    if dashed_line { dashed(color, width, &LINK_DASH) } else { solid(color, width) }
}

/// Size a link's label needs in the layout, with room for its direction arrow.
pub fn label_size(m: &Measured, id: LabelId, dir: Option<bool>) -> (f32, f32) {
    let (w, h) = m.size(id);
    (w + if dir.is_some() { LABEL_ARROW } else { 0.0 } + 2.0, h)
}

fn unit(a: Pt, b: Pt) -> Pt {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l = (dx * dx + dy * dy).sqrt();
    if l < 1e-4 { (0.0, 1.0) } else { (dx / l, dy / l) }
}

/// Point and direction of travel at arc length `d` from the start of the route.
fn along(pts: &[Pt], d: f32) -> (Pt, Pt) {
    let mut left = d;
    for w in pts.windows(2) {
        let len = dist(w[0], w[1]);
        if len >= left {
            return (lerp(w[0], w[1], if len > 0.0 { left / len } else { 0.0 }), unit(w[0], w[1]));
        }
        left -= len;
    }
    let n = pts.len();
    (pts[n - 1], if n > 1 { unit(pts[n - 2], pts[n - 1]) } else { (0.0, 1.0) })
}

fn length(pts: &[Pt]) -> f32 {
    pts.windows(2).map(|w| dist(w[0], w[1])).sum()
}

/// Everything a link needs to be drawn besides its route.
pub struct LinkDraw<'a> {
    pub arrow: &'a Arrow,
    pub color: &'a str,
    pub label: Option<LabelId>,
    pub label_dir: Option<bool>,
    pub from_card: Option<LabelId>,
    pub to_card: Option<LabelId>,
    pub shapes: &'a Shapes,
}

pub fn draw_link(scene: &mut Scene, route: &Route, l: &LinkDraw, m: &Measured) {
    let a = l.arrow;
    if a.hidden || route.segs.is_empty() {
        return;
    }
    let width = a.width.unwrap_or(1.0);
    let color = a.color.as_deref().unwrap_or(l.color);
    let stroke = if a.dashed { dashed(color, width, &l.shapes.link_dash) } else { solid(color, width) };
    let (tail, head) = (deco_head(a.tail), deco_head(a.head));

    let segs = &route.segs;
    let pts = route.flatten();
    let first = segs[0];
    let last = segs[segs.len() - 1];
    // Directions pointing out of the first node and into the last one.
    let start_dir = unit(first[1], first[0]);
    let end_dir = unit(last[2], last[3]);
    let start_cut = head_inset(tail);
    let end_cut = head_inset(head);
    let mut path = PathBuilder::new();
    let p0 = (first[0].0 - start_dir.0 * start_cut, first[0].1 - start_dir.1 * start_cut);
    path.move_to(p0.0, p0.1);
    let n = segs.len();
    for (i, s) in segs.iter().enumerate() {
        let (mut c2, mut e) = (s[2], s[3]);
        if i == n - 1 {
            e = (e.0 - end_dir.0 * end_cut, e.1 - end_dir.1 * end_cut);
            c2 = (c2.0 - end_dir.0 * end_cut * 0.5, c2.1 - end_dir.1 * end_cut * 0.5);
        }
        let mut c1 = s[1];
        if i == 0 {
            c1 = (c1.0 - start_dir.0 * start_cut * 0.5, c1.1 - start_dir.1 * start_cut * 0.5);
        }
        path.cubic_to(c1.0, c1.1, c2.0, c2.1, e.0, e.1);
    }
    scene.push_path(path, Style::new().stroke(stroke));
    arrowhead(scene, first[0], start_dir, tail, color, width);
    arrowhead(scene, last[3], end_dir, head, color, width);

    if let Some(id) = l.label {
        let (cx, cy) = route.label.unwrap_or_else(|| along(&pts, length(&pts) / 2.0).0);
        let (w, h) = label_size(m, id, l.label_dir);
        let arrow_w = if l.label_dir.is_some() { LABEL_ARROW } else { 0.0 };
        let text_w = m.size(id).0;
        let x = cx - w / 2.0 + arrow_w;
        match &l.shapes.label_background {
            None => m.text_at(scene, id, x + text_w / 2.0 + 1.0, cy - h / 2.0, HAlign::Center),
            Some(bg) => {
                let (tw, th) = m.size(id);
                // Only Mermaid sets a label background, and it centers multi-line labels.
                let tb = m.textbox(id, x + 1.0, cy - h / 2.0, tw, th, HAlign::Center, VAlign::Top);
                scene.push_text(tb.background(bg.clone()));
            }
        }
        if let Some(forward) = l.label_dir {
            let (_, dir) = along(&pts, length(&pts) / 2.0);
            let dir = if forward { dir } else { (-dir.0, -dir.1) };
            // The arrow always sits left of the text, pointing the way the link runs.
            let c = (cx - w / 2.0 + 4.0, cy);
            let d = if dir.0.abs() > dir.1.abs() { (dir.0.signum(), 0.0) } else { (0.0, dir.1.signum()) };
            let n = (-d.1, d.0);
            let pts = [(c.0 + d.0 * 4.0, c.1 + d.1 * 4.0), (c.0 - d.0 * 4.0 + n.0 * 3.5, c.1 - d.1 * 4.0 + n.1 * 3.5), (c.0 - d.0 * 4.0 - n.0 * 3.5, c.1 - d.1 * 4.0 - n.1 * 3.5)];
            scene.push_polygon(&pts, Style::new().fill(Paint::solid(color)));
        }
    }
    // The layout puts multiplicities where they collide with nothing; routes made
    // elsewhere fall back to a spot next to the end.
    let total = length(&pts);
    for (id, spot, inset, at_end) in [(l.from_card, route.tail_label, start_cut, false), (l.to_card, route.head_label, end_cut, true)] {
        let Some(id) = id else { continue };
        match spot {
            Some(c) => {
                let (_, h) = m.size(id);
                m.text_at(scene, id, c.0, c.1 - h / 2.0, HAlign::Center);
            }
            None => card(scene, m, id, &pts, inset, at_end, total),
        }
    }
}

/// A multiplicity next to the end of a link, on the side that keeps it clear of the line.
fn card(scene: &mut Scene, m: &Measured, id: LabelId, pts: &[Pt], inset: f32, at_end: bool, total: f32) {
    let (w, h) = m.size(id);
    let vertical_run = |dir: Pt| dir.1.abs() > dir.0.abs();
    // On a short link both cardinalities have to share the line.
    let probe = (12.0 + inset).min(total * 0.4);
    let d = if at_end { (total - probe).max(0.0) } else { probe.min(total) };
    let (p, dir) = along(pts, d);
    // Labels sit right of vertical lines and above horizontal ones, so cardinalities
    // take the other side.
    let mut n = (-dir.1, dir.0);
    if vertical_run(dir) {
        if n.0 > 0.0 {
            n = (-n.0, -n.1);
        }
    } else if n.1 < 0.0 {
        n = (-n.0, -n.1);
    }
    let off = n.0.abs() * w / 2.0 + n.1.abs() * h / 2.0 + 3.0;
    let along_shift = if vertical_run(dir) { h / 2.0 } else { w / 2.0 } * if at_end { -1.0 } else { 1.0 };
    let sign = if vertical_run(dir) { dir.1.signum() } else { dir.0.signum() };
    let c = (p.0 + n.0 * off + if vertical_run(dir) { 0.0 } else { along_shift * sign }, p.1 + n.1 * off + if vertical_run(dir) { along_shift * sign } else { 0.0 });
    m.text_at(scene, id, c.0, c.1 - h / 2.0, HAlign::Center);
}

// ---- notes ---------------------------------------------------------------------------------

pub const NOTE_PAD: (f32, f32) = (7.0, 5.0);

pub fn note_size(m: &Measured, id: LabelId) -> (f32, f32) {
    let (w, h) = m.size(id);
    // The folded corner eats some of the right side.
    (w + NOTE_PAD.0 + 13.0, h + 2.0 * NOTE_PAD.1)
}

pub fn draw_note(scene: &mut Scene, m: &Measured, id: LabelId, r: &Rect, fill: Option<Paint>, skin: &Skin) {
    let fill = fill.unwrap_or_else(|| skin.fill(&["note"], skin::NOTE_FILL));
    let border = skin.border(&["note"], skin::BORDER);
    note_shape(scene, skin.style.note_kind, r.x, r.y, r.w, r.h, fill, &border, 1.0);
    m.text_at(scene, id, r.x + NOTE_PAD.0, r.y + NOTE_PAD.1, HAlign::Left);
}

/// The dashed line from a note on a link to the link: ends at the note's nearest point.
pub fn note_link(mid: Pt, r: &Rect) -> [Pt; 2] {
    [mid, (mid.0.clamp(r.x, r.right()), mid.1.clamp(r.y, r.bottom()))]
}

/// A dashed connector between two outlines.
pub fn note_connector(scene: &mut Scene, route: &Route, skin: &Skin) {
    let border = skin.border(&["note"], skin::BORDER);
    scene.push_polyline(&route.flatten(), dashed(&border, 1.0, &NOTE_DASH));
}

// ---- containers ----------------------------------------------------------------------------

/// Space a container reserves around its members and the smallest outline it can have.
pub fn group_metrics(style: GroupStyle, title: Option<(f32, f32)>) -> ([f32; 4], (f32, f32)) {
    let (tw, th) = title.unwrap_or((0.0, 0.0));
    let has = title.is_some();
    match style {
        GroupStyle::Package | GroupStyle::Folder | GroupStyle::Frame => {
            let tab_h = if has { th + 8.0 } else { 0.0 };
            ([12.0, tab_h + 12.0, 12.0, 12.0], (if has { tw + 30.0 } else { 40.0 }, tab_h + 36.0))
        }
        GroupStyle::Rectangle => ([12.0, if has { th + 14.0 } else { 12.0 }, 12.0, 12.0], (tw + 24.0, th + 36.0)),
        GroupStyle::Node => ([12.0, if has { th + 22.0 } else { 22.0 }, 22.0, 12.0], (tw + 44.0, th + 52.0)),
        GroupStyle::Cloud => ([22.0, if has { th + 24.0 } else { 22.0 }, 22.0, 20.0], (tw + 60.0, th + 70.0)),
        GroupStyle::Database => ([14.0, if has { th + 26.0 } else { 26.0 }, 14.0, 14.0], (tw + 30.0, th + 60.0)),
    }
}

/// Whether a container's title is drawn in the middle of its top edge.
pub fn title_centered(style: GroupStyle) -> bool {
    !matches!(style, GroupStyle::Package | GroupStyle::Folder | GroupStyle::Frame)
}

pub struct GroupLook {
    pub fill: Option<Paint>,
    pub tab_fill: Paint,
    pub border: String,
    pub width: f32,
    pub dashed: bool,
}

/// `title_dx` moves a centred title sideways, away from lines that cross it. Returns
/// where the title went, for the layout checks.
pub fn draw_group(scene: &mut Scene, m: &Measured, style: GroupStyle, r: &Rect, title: Option<LabelId>, title_dx: f32, look: &GroupLook) -> Option<Rect> {
    let stroke = || outline(&look.border, look.width, look.dashed);
    let body = |fill: Option<Paint>| {
        let mut s = Style::new().stroke(stroke());
        if let Some(f) = fill {
            s = s.fill(f);
        }
        s
    };
    let tsize = title.map(|t| m.size(t));
    let at = match style {
        GroupStyle::Package | GroupStyle::Folder | GroupStyle::Frame => {
            let (tw, th) = tsize.unwrap_or((0.0, 0.0));
            let tab_h = if title.is_some() { th + 8.0 } else { 0.0 };
            let tab_w = if title.is_some() { tw + 20.0 } else { 0.0 };
            if style == GroupStyle::Frame {
                scene.push_rect(r.x, r.y, r.w, r.h, 0.0, body(look.fill.clone()));
                if title.is_some() {
                    let tab = [(r.x, r.y), (r.x + tab_w, r.y), (r.x + tab_w, r.y + tab_h - 8.0), (r.x + tab_w - 8.0, r.y + tab_h), (r.x, r.y + tab_h)];
                    scene.push_polygon(&tab, Style::new().fill(look.tab_fill.clone()).stroke(solid(&look.border, look.width)));
                }
            } else {
                scene.push_rect(r.x, r.y + tab_h, r.w, r.h - tab_h, 0.0, body(look.fill.clone()));
                if title.is_some() {
                    let bevel = if style == GroupStyle::Folder { 6.0 } else { 0.0 };
                    let tab = [(r.x, r.y + tab_h), (r.x, r.y), (r.x + tab_w - bevel, r.y), (r.x + tab_w, r.y + bevel), (r.x + tab_w, r.y + tab_h)];
                    scene.push_polygon(&tab, Style::new().fill(look.tab_fill.clone()).stroke(solid(&look.border, look.width)));
                }
            }
            (r.x + tab_w / 2.0, r.y + 4.0)
        }
        GroupStyle::Rectangle => {
            scene.push_rect(r.x, r.y, r.w, r.h, 0.0, body(look.fill.clone()));
            (r.x + r.w / 2.0 + title_dx, r.y + 6.0)
        }
        GroupStyle::Node => {
            box3d(scene, r, 10.0, look.fill.clone().unwrap_or_else(|| Paint::solid("#00000000")), &look.border, look.width);
            (r.x + (r.w - 10.0) / 2.0 + title_dx, r.y + 16.0)
        }
        GroupStyle::Cloud => {
            scene.push_path(cloud_path(r), body(look.fill.clone()));
            (r.x + r.w / 2.0 + title_dx, r.y + 16.0)
        }
        GroupStyle::Database => {
            cylinder(scene, r, 8.0, look.fill.clone().unwrap_or_else(|| Paint::solid("#00000000")), &look.border, look.width);
            (r.x + r.w / 2.0 + title_dx, r.y + 22.0)
        }
    };
    let t = title?;
    m.text_at(scene, t, at.0, at.1, HAlign::Center);
    let (w, h) = m.size(t);
    Some(Rect::new(at.0 - w / 2.0, at.1, w, h))
}

/// A box with a top and a right face, `d` deep.
pub fn box3d(scene: &mut Scene, r: &Rect, d: f32, fill: Paint, border: &str, lw: f32) {
    let st = Style::new().fill(fill).stroke(solid(border, lw));
    let (x, y, w, h) = (r.x, r.y, r.w, r.h);
    scene.push_polygon(&[(x, y + d), (x + d, y), (x + w, y), (x + w - d, y + d)], st.clone());
    scene.push_polygon(&[(x + w - d, y + d), (x + w, y), (x + w, y + h - d), (x + w - d, y + h)], st.clone());
    scene.push_rect(x, y + d, w - d, h - d, 0.0, st);
}

/// A cylinder, `ry` is the half height of its lids.
pub fn cylinder(scene: &mut Scene, r: &Rect, ry: f32, fill: Paint, border: &str, lw: f32) {
    let k = 0.5523;
    let (x, y, w, h) = (r.x, r.y, r.w, r.h);
    let (rx, cx) = (w / 2.0, x + w / 2.0);
    let mut outline = PathBuilder::new();
    outline.move_to(x, y + ry);
    outline.cubic_to(x, y + ry - ry * k, cx - rx * k, y, cx, y);
    outline.cubic_to(cx + rx * k, y, x + w, y + ry - ry * k, x + w, y + ry);
    outline.line_to(x + w, y + h - ry);
    outline.cubic_to(x + w, y + h - ry + ry * k, cx + rx * k, y + h, cx, y + h);
    outline.cubic_to(cx - rx * k, y + h, x, y + h - ry + ry * k, x, y + h - ry);
    outline.close();
    scene.push_path(outline, Style::new().fill(fill).stroke(solid(border, lw)));
    let mut lid = PathBuilder::new();
    lid.move_to(x, y + ry);
    lid.cubic_to(x, y + ry + ry * k, cx - rx * k, y + 2.0 * ry, cx, y + 2.0 * ry);
    lid.cubic_to(cx + rx * k, y + 2.0 * ry, x + w, y + ry + ry * k, x + w, y + ry);
    scene.push_path(lid, Style::new().stroke(solid(border, lw)));
}

/// A scalloped outline inside the rectangle.
pub fn cloud_path(r: &Rect) -> PathBuilder {
    let (cx, cy) = r.center();
    let (rx, ry) = (r.w / 2.0 - 6.0, r.h / 2.0 - 6.0);
    let k = (((rx + ry) * std::f32::consts::PI / 22.0).round() as usize).clamp(8, 28);
    let pt = |i: usize| {
        let a = i as f32 / k as f32 * std::f32::consts::TAU;
        (cx + rx * a.cos(), cy + ry * a.sin())
    };
    let mut p = PathBuilder::new();
    let first = pt(0);
    p.move_to(first.0, first.1);
    for i in 0..k {
        let (a, b) = (pt(i), pt(i + 1));
        let mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
        let out = unit((cx, cy), mid);
        let chord = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
        p.quad_to(mid.0 + out.0 * chord * 0.7, mid.1 + out.1 * chord * 0.7, b.0, b.1);
    }
    p.close();
    p
}

/// A decorated circle with a letter, as in class headers.
pub fn spot(scene: &mut Scene, cx: f32, cy: f32, r: f32, fill: &str, border: &str) {
    scene.push_ellipse(cx - r, cy - r, 2.0 * r, 2.0 * r, Style::new().fill(Paint::solid(fill)).stroke(solid(border, 1.0)));
}

pub fn translate(r: &Rect, dx: f32, dy: f32) -> Rect {
    Rect::new(r.x + dx, r.y + dy, r.w, r.h)
}

/// Padding around the drawn picture so protruding details (generics boxes, labels) stay inside.
const PAD: f32 = 4.0;

/// Bounding box of every item in the scene.
pub fn bounds(s: &Scene) -> (f32, f32, f32, f32) {
    use crate::scene::Item;
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for it in &s.items {
        let (a, b, c, d) = match it {
            Item::Shape(sh) => (sh.bbox[0] - sh.stroke.as_ref().map_or(0.0, |k| k.width / 2.0), sh.bbox[1] - sh.stroke.as_ref().map_or(0.0, |k| k.width / 2.0), sh.bbox[0] + sh.bbox[2], sh.bbox[1] + sh.bbox[3]),
            Item::Text(t) => (t.x, t.y, t.x + t.w, t.y + t.h),
        };
        x0 = x0.min(a);
        y0 = y0.min(b);
        x1 = x1.max(c);
        y1 = y1.max(d);
    }
    if x0 > x1 {
        return (0.0, 0.0, 0.0, 0.0);
    }
    (x0, y0, x1, y1)
}

/// Moves the drawing to the origin plus padding and wraps it as a body. Returns the
/// offset that was applied so callers can translate their own coordinates.
pub fn finish(body: Scene) -> (super::Body, f32, f32) {
    let (x0, y0, x1, y1) = bounds(&body);
    let mut scene = Scene::new(0.0, 0.0);
    scene.items = body.items;
    scene.warnings = body.warnings;
    let (dx, dy) = (PAD - x0, PAD - y0);
    scene.translate(dx, dy);
    (super::Body::new(scene, x1 - x0 + 2.0 * PAD, y1 - y0 + 2.0 * PAD), dx, dy)
}
