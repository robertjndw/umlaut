//! Graph layout shared by the class, state, use case and component diagrams. It knows
//! nothing about UML: callers hand over boxes with a size, clusters, and edges with
//! label sizes and a direction hint, and get back rectangles and Bezier routes.
//!
//! Placement and routing are done by Graphviz's dot (`crate::graphviz`), the same way
//! PlantUML does it: dot sees every box as a fixed-size node and every label as a
//! fixed-size placeholder, and only its coordinates are used. Drawing stays ours.
//!
//! Mapping to dot:
//!
//! - A box is a fixed-size node (`box`, `ellipse` or `diamond` to match its outline, so
//!   dot clips edges where our shape ends). A cluster is a `cluster_N` subgraph; its
//!   title area is a label of exactly the size the caller reserved.
//! - Edge labels and the two end labels (multiplicities) are fixed-size text
//!   placeholders; dot reserves the room and reports where it put them.
//! - Arrow length picks the rank distance like PlantUML: one dash keeps both ends on
//!   the same rank, `n` dashes is `minlen = n - 1`. `-up->` and `-left->` are emitted
//!   as the reversed edge, so dot's top-down and left-to-right order does the work, and
//!   the route is flipped back afterwards.
//! - An edge to a cluster goes to one of its nodes and is clipped at the cluster border
//!   (`lhead`/`ltail`).
//! - Concurrent state regions cannot be expressed as dot clusters (dot would not stack
//!   them), so every region is laid out on its own and the whole stack takes part in
//!   its parent as one box. An edge into a region ends at that box, then continues in a
//!   straight line to the real target.

use crate::graphviz::{self, Placed};
use std::collections::BTreeMap;

pub type Pt = (f32, f32);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn center(&self) -> Pt {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn contains(&self, p: Pt) -> bool {
        p.0 >= self.x && p.0 <= self.right() && p.1 >= self.y && p.1 <= self.bottom()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

impl Dir {
    pub fn opposite(self) -> Dir {
        match self {
            Dir::Up => Dir::Down,
            Dir::Down => Dir::Up,
            Dir::Left => Dir::Right,
            Dir::Right => Dir::Left,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum End {
    Node(usize),
    Cluster(usize),
}

/// Outline of a node, for the shape dot gets and for clipping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Border {
    Rect,
    Ellipse,
    Diamond,
    /// A figure of this width and height at the top of a box with the label below it (an
    /// actor, an interface circle). Dot only knows the box.
    Icon(u16, u16),
    /// A box whose left and right sides lean (hexagon, parallelogram, trapezoid) by this
    /// many pt. Dot and clipping only know the box; the lean keeps line ends off the
    /// corners where the outline has already pulled away from the box.
    Slanted(u16),
}

/// How an edge wants its ends placed. `Auto(len)` is the number of dashes of the arrow:
/// one dash is horizontal, more are vertical by `len - 1` ranks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hint {
    Auto(usize),
    Dir(Dir),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrange {
    /// Members form a graph.
    Graph,
    /// Child clusters stacked on top of each other (concurrent regions).
    StackV,
    /// Child clusters side by side.
    StackH,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Spline,
    Ortho,
    Polyline,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub w: f32,
    pub h: f32,
    pub parent: Option<usize>,
    pub border: Border,
}

#[derive(Debug, Clone)]
pub struct Cluster {
    pub parent: Option<usize>,
    /// Space between the cluster's outline and its members: left, top, right, bottom.
    /// The top one holds the title.
    pub inset: [f32; 4],
    /// Smallest outline, so a title or an empty body still fits.
    pub min: (f32, f32),
    pub arrange: Arrange,
    /// The title is drawn in the middle of the top edge, not at the left, and dot has to
    /// keep lines out of that spot.
    pub title_centered: bool,
}

#[derive(Debug, Clone)]
pub struct Edge {
    pub from: End,
    pub to: End,
    pub hint: Hint,
    /// Size of the label that sits on the line.
    pub label: Option<(f32, f32)>,
    /// Sizes of the labels at the `from` and the `to` end (multiplicities).
    pub tail_label: Option<(f32, f32)>,
    pub head_label: Option<(f32, f32)>,
}

#[derive(Debug, Clone, Default)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub clusters: Vec<Cluster>,
    pub edges: Vec<Edge>,
}

/// Which way the ranks run. PlantUML only has `TB` and `LR` (`left to right direction`);
/// Mermaid can reverse either.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RankDir {
    #[default]
    TB,
    BT,
    LR,
    RL,
}

impl RankDir {
    /// Ranks run across the page rather than down it.
    pub fn lr(self) -> bool {
        matches!(self, RankDir::LR | RankDir::RL)
    }

    /// Ranks run bottom to top or right to left.
    pub fn reverse(self) -> bool {
        matches!(self, RankDir::BT | RankDir::RL)
    }

    fn dot(self) -> &'static str {
        match self {
            RankDir::TB => "TB",
            RankDir::BT => "BT",
            RankDir::LR => "LR",
            RankDir::RL => "RL",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Opts {
    pub dir: RankDir,
    pub style: Style,
    /// Gap between neighbors in a rank.
    pub node_sep: f32,
    /// Gap between ranks.
    pub rank_sep: f32,
}

impl Default for Opts {
    fn default() -> Opts {
        Opts { dir: RankDir::TB, style: Style::Spline, node_sep: 35.0, rank_sep: 60.0 }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Route {
    /// Cubic Bezier segments from `edge.from`'s outline to `edge.to`'s outline; each
    /// segment starts where the previous one ends.
    pub segs: Vec<[Pt; 4]>,
    /// Centre of the edge's label.
    pub label: Option<Pt>,
    /// Centres of the labels at the `from` and `to` end.
    pub tail_label: Option<Pt>,
    pub head_label: Option<Pt>,
}

impl Route {
    pub fn start(&self) -> Pt {
        self.segs.first().map_or((0.0, 0.0), |s| s[0])
    }

    pub fn end(&self) -> Pt {
        self.segs.last().map_or((0.0, 0.0), |s| s[3])
    }

    /// The route as a polyline: the curve sampled finely enough for hit tests and for
    /// drawing thin dashed lines.
    pub fn flatten(&self) -> Vec<Pt> {
        let mut out = Vec::with_capacity(self.segs.len() * 8 + 1);
        if let Some(s) = self.segs.first() {
            out.push(s[0]);
        }
        for s in &self.segs {
            for k in 1..=8 {
                out.push(cubic(s, k as f32 / 8.0));
            }
        }
        out
    }

    /// Half way along the curve.
    pub fn mid(&self) -> Pt {
        point_at(&self.flatten(), 0.5)
    }

    fn shift(&mut self, dx: f32, dy: f32) {
        let mv = |p: &mut Pt| *p = (p.0 + dx, p.1 + dy);
        self.segs.iter_mut().flatten().for_each(mv);
        [&mut self.label, &mut self.tail_label, &mut self.head_label].into_iter().flatten().for_each(mv);
    }

    fn reversed(mut self) -> Route {
        self.segs.reverse();
        for s in &mut self.segs {
            s.reverse();
        }
        std::mem::swap(&mut self.tail_label, &mut self.head_label);
        self
    }

    fn line(a: Pt, b: Pt) -> Route {
        Route { segs: vec![[a, lerp(a, b, 1.0 / 3.0), lerp(a, b, 2.0 / 3.0), b]], ..Route::default() }
    }
}

#[derive(Debug, Clone)]
pub struct Layout {
    pub nodes: Vec<Rect>,
    pub clusters: Vec<Rect>,
    pub routes: Vec<Route>,
    pub w: f32,
    pub h: f32,
    /// Node boxes and every placed label, what a note on a link has to keep clear of.
    obstacles: Vec<Rect>,
    /// `routes` flattened, for the hit tests that place titles and notes.
    polys: Vec<Vec<Pt>>,
}

// ---- geometry ------------------------------------------------------------------------------

pub(crate) fn dist(a: Pt, b: Pt) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

pub(crate) fn lerp(a: Pt, b: Pt, t: f32) -> Pt {
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
}

fn cubic(s: &[Pt; 4], t: f32) -> Pt {
    let u = 1.0 - t;
    let f = |a: f32, b: f32, c: f32, d: f32| u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t * d;
    (f(s[0].0, s[1].0, s[2].0, s[3].0), f(s[0].1, s[1].1, s[2].1, s[3].1))
}

/// Where the ray from the centre of `r` towards `toward` leaves the outline.
pub fn clip(r: &Rect, border: Border, toward: Pt) -> Pt {
    let (cx, cy) = r.center();
    let (hw, hh) = (r.w / 2.0, r.h / 2.0);
    let (dx, dy) = (toward.0 - cx, toward.1 - cy);
    if dx.abs() < 1e-4 && dy.abs() < 1e-4 {
        return (cx, r.y);
    }
    if let Border::Icon(iw, ih) = border {
        // Below the figure the line has to reach the label's side of the box instead.
        if toward.1 <= r.y + ih as f32 {
            let (fx, fy) = (cx, r.y + ih as f32 / 2.0);
            let (dx, dy) = (toward.0 - fx, toward.1 - fy);
            let t = 1.0 / ((dx / (iw as f32 / 2.0)).powi(2) + (dy / (ih as f32 / 2.0)).powi(2)).sqrt().max(1e-3);
            return (fx + dx * t, fy + dy * t);
        }
    }
    let t = match border {
        Border::Rect | Border::Icon(..) | Border::Slanted(_) => {
            let tx = if dx.abs() < 1e-6 { f32::MAX } else { hw / dx.abs() };
            let ty = if dy.abs() < 1e-6 { f32::MAX } else { hh / dy.abs() };
            tx.min(ty)
        }
        Border::Ellipse => 1.0 / ((dx / hw.max(1e-3)).powi(2) + (dy / hh.max(1e-3)).powi(2)).sqrt(),
        Border::Diamond => 1.0 / (dx.abs() / hw.max(1e-3) + dy.abs() / hh.max(1e-3)),
    };
    (cx + dx * t, cy + dy * t)
}

/// Where a label of size `size` centred at `c` has to move, across the direction the
/// ranks run in, to sit `LABEL_GAP` from the route along its whole height. `None` if it
/// is close enough already or the route never passes its centre line.
fn snap_to_route(poly: &[Pt], c: Pt, size: (f32, f32), lr: bool) -> Option<Pt> {
    // Work in a frame where ranks run down, so only one axis is handled.
    let flip = |p: Pt| if lr { (p.1, p.0) } else { p };
    let (c, (w, h)) = (flip(c), if lr { (size.1, size.0) } else { size });
    let cross_at = |y: f32| {
        poly.windows(2)
            .filter_map(|s| {
                let (a, b) = (flip(s[0]), flip(s[1]));
                (y >= a.1.min(b.1) && y <= a.1.max(b.1) && a.1 != b.1).then(|| a.0 + (b.0 - a.0) * (y - a.1) / (b.1 - a.1))
            })
            .min_by(|x, y| (x - c.0).abs().total_cmp(&(y - c.0).abs()))
    };
    let right = c.0 >= cross_at(c.1)?;
    // A slanted line comes closest to the label at one of its two ends.
    let edge = [c.1 - h / 2.0, c.1, c.1 + h / 2.0].into_iter().filter_map(cross_at).reduce(|a, b| if right { a.max(b) } else { a.min(b) })?;
    let want = edge + (w / 2.0 + LABEL_GAP) * if right { 1.0 } else { -1.0 };
    ((want - c.0).abs() > LABEL_GAP).then(|| flip((want, c.1)))
}

/// The point a fraction `t` of the way along a flattened route.
fn point_at(poly: &[Pt], t: f32) -> Pt {
    let total: f32 = poly.windows(2).map(|w| dist(w[0], w[1])).sum();
    let mut left = total * t;
    for w in poly.windows(2) {
        let len = dist(w[0], w[1]);
        if len >= left && len > 0.0 {
            return lerp(w[0], w[1], left / len);
        }
        left -= len;
    }
    poly.last().copied().unwrap_or((0.0, 0.0))
}

/// Where a label of `size` sits beside a flattened route, `LABEL_GAP` off the line: the
/// point at `t` along the route (through `map`) and the offset to either side of it.
fn beside_at(poly: &[Pt], t: f32, map: impl Fn(f32) -> f32, (w, h): (f32, f32)) -> (Pt, Pt) {
    let (a, b) = (point_at(poly, map((t - 0.02).max(0.0))), point_at(poly, map(t + 0.02)));
    let d = (b.0 - a.0, b.1 - a.1);
    let len = (d.0 * d.0 + d.1 * d.1).sqrt().max(1e-3);
    let n = (-d.1 / len, d.0 / len);
    let reach = n.0.abs() * w / 2.0 + n.1.abs() * h / 2.0 + LABEL_GAP;
    (point_at(poly, map(t)), (n.0 * reach, n.1 * reach))
}

/// Distance between a rectangle and a flattened route (to the sampled points, zero when
/// a segment runs through it).
fn rect_gap(r: &Rect, poly: &[Pt]) -> f32 {
    if poly.windows(2).any(|w| seg_hits_rect(w[0], w[1], r)) {
        return 0.0;
    }
    poly.iter().map(|p| dist(*p, (p.0.clamp(r.x, r.right()), p.1.clamp(r.y, r.bottom())))).fold(f32::MAX, f32::min)
}

fn rect_at(c: Pt, w: f32, h: f32) -> Rect {
    Rect::new(c.0 - w / 2.0, c.1 - h / 2.0, w, h)
}

pub(super) fn rects_hit(a: &Rect, b: &Rect) -> bool {
    a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
}

/// Whether the segment touches the rectangle (Liang-Barsky).
fn seg_hits_rect(a: Pt, b: Pt, r: &Rect) -> bool {
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    let d = (b.0 - a.0, b.1 - a.1);
    for (p, q) in [(-d.0, a.0 - r.x), (d.0, r.right() - a.0), (-d.1, a.1 - r.y), (d.1, r.bottom() - a.1)] {
        if p.abs() < 1e-6 {
            if q < 0.0 {
                return false;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

fn hits_polys(r: &Rect, polys: &[Vec<Pt>]) -> usize {
    polys.iter().map(|p| p.windows(2).filter(|w| seg_hits_rect(w[0], w[1], r)).count()).sum()
}

impl Layout {
    /// How far a container's title, centred at the top of `r`, has to move sideways to
    /// keep clear of the lines that enter the container through its top edge: the
    /// smallest shift that crosses none, else none.
    pub fn title_dx(&self, r: &Rect, size: (f32, f32)) -> f32 {
        // The title sits within this much of the top edge in every container style.
        let band = |dx: f32| Rect::new(r.center().0 + dx - size.0 / 2.0 - 2.0, r.y, size.0 + 4.0, size.1 + 28.0);
        let room = (r.w / 2.0 - size.0 / 2.0 - 6.0).max(0.0);
        let free = |dx: f32| hits_polys(&band(dx), &self.polys) == 0;
        (0..=16).flat_map(|k| [1.0f32, -1.0].map(|s| s * k as f32 * 8.0)).filter(|dx| dx.abs() <= room).find(|&dx| free(dx)).unwrap_or(0.0)
    }

    /// A free spot of the given size near `anchor` (a point on a link), clear of nodes,
    /// labels, lines and the rectangles in `taken`; the nearest on the right of the
    /// link, else on the left, else the least bad one.
    pub fn place_note(&self, anchor: Pt, w: f32, h: f32, taken: &[Rect]) -> Rect {
        let mut best: Option<(usize, Rect)> = None;
        for dx in [30.0f32, 46.0, 70.0, 100.0, 140.0] {
            for left in [false, true] {
                for dy in [0.0f32, -(h / 2.0 + 8.0), h / 2.0 + 8.0] {
                    let x = if left { anchor.0 - dx - w } else { anchor.0 + dx };
                    let r = Rect::new(x, anchor.1 + dy - h / 2.0, w, h);
                    let probe = Rect::new(r.x - 2.0, r.y - 2.0, r.w + 4.0, r.h + 4.0);
                    let cost = self.obstacles.iter().chain(taken).filter(|t| rects_hit(t, &probe)).count() * 10 + hits_polys(&probe, &self.polys);
                    if best.is_none_or(|(b, _)| cost < b) {
                        best = Some((cost, r));
                    }
                    if cost == 0 {
                        return r;
                    }
                }
            }
        }
        best.map_or(Rect::new(anchor.0 + 30.0, anchor.1 - h / 2.0, w, h), |(_, r)| r)
    }
}

// ---- dot input -----------------------------------------------------------------------------

/// Points to inches, the unit dot's `width`, `height`, `nodesep` and `ranksep` use.
fn inches(pt: f32) -> String {
    format!("{:.4}", pt / 72.0)
}

/// How dot spells a label we measured ourselves: the text layout engine in
/// graphviz/shim.c reads the size straight from the string.
fn sized(w: f32, h: f32) -> String {
    format!("{:.2},{:.2}", w.max(0.5), h.max(0.5))
}

/// Gap dot adds around a cluster label (4 pt of padding per side, `GAP` in Graphviz's
/// input.c), which the requested title height has to make up for.
const LABEL_PAD: f32 = 8.0;
const LABEL_PAD_X: f32 = 16.0;
/// Space kept between an edge label and its line.
const LABEL_GAP: f32 = 3.0;
/// Distance an end label keeps from the end of its line, where decorations are drawn.
const END_CLEARANCE: f32 = 12.0;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Origin {
    /// A node of the caller's graph.
    Node(usize),
    /// A concurrent stack that takes part in this graph as one box.
    Block(usize),
    /// Stands in for an empty cluster, which dot would drop.
    Phantom,
}

struct DotNode {
    w: f32,
    h: f32,
    border: Border,
    parent: Option<usize>,
    origin: Origin,
}

struct DotCluster {
    /// Index into the caller's `Graph::clusters`.
    id: usize,
    parent: Option<usize>,
}

struct DotEdge {
    /// Index into the caller's `Graph::edges`.
    id: usize,
    tail: usize,
    head: usize,
    ltail: Option<usize>,
    lhead: Option<usize>,
    minlen: usize,
    /// Dot's tail is our `to`: the route has to be flipped back.
    flipped: bool,
    label: Option<(f32, f32)>,
    tail_label: Option<(f32, f32)>,
    head_label: Option<(f32, f32)>,
}

#[derive(Default)]
struct DotGraph {
    nodes: Vec<DotNode>,
    clusters: Vec<DotCluster>,
    edges: Vec<DotEdge>,
    /// Pairs that have to share a rank (one-dash arrows), within their common cluster.
    same: Vec<(usize, usize)>,
}

impl DotGraph {
    fn within(&self, mut node: Option<usize>, cluster: usize) -> bool {
        while let Some(c) = node {
            if c == cluster {
                return true;
            }
            node = self.clusters[c].parent;
        }
        false
    }

    fn node_within(&self, node: usize, cluster: usize) -> bool {
        self.within(self.nodes[node].parent, cluster)
    }
}

/// `rank=same` groups: pairs joined by one-dash arrows, merged so a chain `A -> B -> C`
/// is one row, keyed by the cluster that holds them.
fn same_rank_groups(dg: &DotGraph) -> BTreeMap<Option<usize>, Vec<Vec<usize>>> {
    let mut group: Vec<usize> = (0..dg.nodes.len()).collect();
    fn find(group: &mut [usize], mut x: usize) -> usize {
        while group[x] != x {
            group[x] = group[group[x]];
            x = group[x];
        }
        x
    }
    for &(a, b) in &dg.same {
        let (ra, rb) = (find(&mut group, a), find(&mut group, b));
        group[ra.max(rb)] = ra.min(rb);
    }
    let mut members: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for &(a, b) in &dg.same {
        for x in [a, b] {
            let root = find(&mut group, x);
            let list = members.entry(root).or_default();
            if !list.contains(&x) {
                list.push(x);
            }
        }
    }
    let mut out: BTreeMap<Option<usize>, Vec<Vec<usize>>> = BTreeMap::new();
    for (_, mut list) in members {
        list.sort_unstable();
        let parent = dg.nodes[list[0]].parent;
        out.entry(parent).or_default().push(list);
    }
    out
}

fn node_attrs(n: &DotNode) -> String {
    let shape = match n.border {
        Border::Ellipse => "ellipse",
        Border::Diamond => "diamond",
        Border::Rect | Border::Icon(..) | Border::Slanted(_) => "box",
    };
    let style = if n.origin == Origin::Phantom { " style=invis" } else { "" };
    format!("[shape={shape} width={} height={}{style}]", inches(n.w.max(1.0)), inches(n.h.max(1.0)))
}

fn write_cluster(out: &mut String, dg: &DotGraph, clusters: &[Cluster], same: &BTreeMap<Option<usize>, Vec<Vec<usize>>>, which: Option<usize>, depth: usize) {
    let pad = "  ".repeat(depth);
    if let Some(c) = which {
        let cl = &clusters[dg.clusters[c].id];
        // dot keeps a fixed gap to the members on every side; the title area is the label.
        let margin = cl.inset[0].max(cl.inset[2]).max(cl.inset[3]).max(1.0).round();
        out.push_str(&format!("{pad}subgraph cluster_{c} {{\n{pad}  margin={margin}\n"));
        let title_h = cl.inset[1] - margin - LABEL_PAD;
        if title_h > 0.0 {
            let w = (cl.min.0 - LABEL_PAD_X).max(1.0);
            out.push_str(&format!("{pad}  label=\"{}\" labeljust={} labelloc=t\n", sized(w, title_h), if cl.title_centered { 'c' } else { 'l' }));
        }
    }
    for (i, n) in dg.nodes.iter().enumerate().filter(|(_, n)| n.parent == which) {
        out.push_str(&format!("{pad}  n{i} {}\n", node_attrs(n)));
    }
    for group in same.get(&which).into_iter().flatten() {
        let names: Vec<String> = group.iter().map(|i| format!("n{i}")).collect();
        out.push_str(&format!("{pad}  {{ rank=same {} }}\n", names.join(" ")));
    }
    for (c, _) in dg.clusters.iter().enumerate().filter(|(_, c)| c.parent == which) {
        write_cluster(out, dg, clusters, same, Some(c), depth + 1);
    }
    if which.is_some() {
        out.push_str(&format!("{pad}}}\n"));
    }
}

fn dot_text(dg: &DotGraph, clusters: &[Cluster], o: &Opts, compound: bool) -> String {
    let splines = match o.style {
        Style::Spline => "spline",
        Style::Ortho => "ortho",
        Style::Polyline => "polyline",
    };
    let mut out = String::from("digraph G {\n");
    out.push_str(&format!(
        "  graph [rankdir={} nodesep={} ranksep={} splines={splines} compound={compound} remincross=true]\n",
        o.dir.dot(),
        inches(o.node_sep),
        inches(o.rank_sep)
    ));
    out.push_str("  node [fixedsize=true margin=0 label=\"\"]\n  edge [arrowhead=none arrowtail=none]\n");
    let same = if compound { same_rank_groups(dg) } else { BTreeMap::new() };
    write_cluster(&mut out, dg, clusters, &same, None, 0);
    for e in &dg.edges {
        let mut attrs = vec![format!("comment=\"{}\"", e.id), format!("minlen={}", e.minlen)];
        if let Some((w, h)) = e.label {
            attrs.push(format!("label=\"{}\"", sized(w + 4.0, h + 6.0)));
        }
        // Dot parks end labels flush against the line's end; padding the placeholder
        // keeps the drawn text off the line and the node.
        let padded = |(w, h): (f32, f32)| sized(w + 8.0, h + 6.0);
        if let Some(size) = e.tail_label {
            attrs.push(format!("taillabel=\"{}\"", padded(size)));
        }
        if let Some(size) = e.head_label {
            attrs.push(format!("headlabel=\"{}\"", padded(size)));
        }
        if compound {
            if let Some(c) = e.ltail {
                attrs.push(format!("ltail=cluster_{c}"));
            }
            if let Some(c) = e.lhead {
                attrs.push(format!("lhead=cluster_{c}"));
            }
        }
        out.push_str(&format!("  n{} -> n{} [{}]\n", e.tail, e.head, attrs.join(" ")));
    }
    out.push_str("}\n");
    out
}

// ---- scopes: the graph, and every concurrent region on its own ------------------------------

/// Everything placed inside one scope, relative to the scope's top left corner.
#[derive(Default)]
struct Local {
    nodes: Vec<(usize, Rect)>,
    clusters: Vec<(usize, Rect)>,
    routes: Vec<(usize, Route)>,
    w: f32,
    h: f32,
}

impl Local {
    fn shift(&mut self, dx: f32, dy: f32) {
        for (_, r) in self.nodes.iter_mut().chain(self.clusters.iter_mut()) {
            r.x += dx;
            r.y += dy;
        }
        for (_, r) in &mut self.routes {
            r.shift(dx, dy);
        }
    }

    fn absorb(&mut self, mut other: Local, dx: f32, dy: f32) {
        other.shift(dx, dy);
        self.nodes.append(&mut other.nodes);
        self.clusters.append(&mut other.clusters);
        self.routes.append(&mut other.routes);
    }
}

/// An edge after its ends were moved up to the scope where both are visible.
#[derive(Clone, Copy)]
struct Lift {
    scope: usize,
    from: End,
    to: End,
    /// Both ends sit in different regions of one stack, which no dot graph contains.
    across: bool,
}

struct Solver<'a> {
    g: &'a Graph,
    o: &'a Opts,
    /// Scope of every cluster and node: 0 for the top level, `r + 1` for the inside of
    /// region cluster `r`.
    cscope: Vec<usize>,
    nscope: Vec<usize>,
    lifts: Vec<Lift>,
}

impl<'a> Solver<'a> {
    fn new(g: &'a Graph, o: &'a Opts) -> Solver<'a> {
        let mut s = Solver { g, o, cscope: vec![0; g.clusters.len()], nscope: vec![0; g.nodes.len()], lifts: Vec::new() };
        for c in 0..g.clusters.len() {
            s.cscope[c] = s.scope_of(g.clusters[c].parent);
        }
        for i in 0..g.nodes.len() {
            s.nscope[i] = s.scope_of(g.nodes[i].parent);
        }
        s.lifts = g.edges.iter().map(|e| s.lift(e)).collect();
        s
    }

    fn is_stack(&self, c: usize) -> bool {
        self.g.clusters[c].arrange != Arrange::Graph
    }

    fn is_region(&self, c: usize) -> bool {
        self.g.clusters[c].parent.is_some_and(|p| self.is_stack(p))
    }

    /// Scope of something whose parent cluster is `parent`: that of the innermost
    /// region around it.
    fn scope_of(&self, mut parent: Option<usize>) -> usize {
        while let Some(c) = parent {
            if self.is_region(c) {
                return c + 1;
            }
            parent = self.g.clusters[c].parent;
        }
        0
    }

    /// A region belongs to its stack as far as the outside is concerned.
    fn norm(&self, mut e: End) -> End {
        while let End::Cluster(c) = e {
            match self.g.clusters[c].parent {
                Some(p) if self.is_stack(p) => e = End::Cluster(p),
                _ => break,
            }
        }
        e
    }

    /// The scopes an end lies in, outermost first.
    fn chain(&self, e: End) -> Vec<usize> {
        let mut parent = match e {
            End::Node(i) => self.g.nodes[i].parent,
            End::Cluster(c) => self.g.clusters[c].parent,
        };
        let mut out = vec![0];
        while let Some(c) = parent {
            if self.is_region(c) {
                out.push(c + 1);
            }
            parent = self.g.clusters[c].parent;
        }
        out[1..].reverse();
        out
    }

    fn lift(&self, e: &Edge) -> Lift {
        let (a, b) = (self.norm(e.from), self.norm(e.to));
        let (ca, cb) = (self.chain(a), self.chain(b));
        let k = ca.iter().zip(&cb).take_while(|(x, y)| x == y).count().max(1);
        let up = |chain: &[usize], x: End| match chain.get(k) {
            Some(&region) => End::Cluster(self.g.clusters[region - 1].parent.unwrap_or(0)),
            None => x,
        };
        let (from, to) = (up(&ca, a), up(&cb, b));
        let across = from == to && (ca.len() > k || cb.len() > k) && ca.get(k) != cb.get(k);
        Lift { scope: ca[k - 1], from, to, across }
    }

    /// The nearest enclosing cluster that is an ordinary dot cluster in `scope`.
    fn dot_parent(&self, mut parent: Option<usize>, scope: usize) -> Option<usize> {
        while let Some(c) = parent {
            if self.is_region(c) && c + 1 == scope {
                return None;
            }
            if self.cscope[c] == scope && !self.is_stack(c) && !self.is_region(c) {
                return Some(c);
            }
            parent = self.g.clusters[c].parent;
        }
        None
    }

    fn block_size(&self, s: usize, regions: &[(usize, Local)]) -> (f32, f32) {
        let cl = &self.g.clusters[s];
        let vertical = cl.arrange == Arrange::StackV;
        let (mut along, mut across) = (0.0f32, 0.0f32);
        for (r, local) in regions {
            let ri = &self.g.clusters[*r];
            let (w, h) = (local.w + ri.inset[0] + ri.inset[2], local.h + ri.inset[1] + ri.inset[3]);
            let (w, h) = (w.max(ri.min.0), h.max(ri.min.1));
            let (a, c) = if vertical { (h, w) } else { (w, h) };
            along += a;
            across = across.max(c);
        }
        let (w, h) = if vertical { (across, along) } else { (along, across) };
        ((w + cl.inset[0] + cl.inset[2]).max(cl.min.0), (h + cl.inset[1] + cl.inset[3]).max(cl.min.1))
    }

    /// Puts the regions of stack `s` into its box `b`; the regions share the space the
    /// box has beyond what they need.
    fn place_regions(&self, s: usize, b: Rect, regions: Vec<(usize, Local)>, out: &mut Local) {
        let cl = &self.g.clusters[s];
        let vertical = cl.arrange == Arrange::StackV;
        let (ax, ay) = (b.x + cl.inset[0], b.y + cl.inset[1]);
        let (aw, ah) = (b.w - cl.inset[0] - cl.inset[2], b.h - cl.inset[1] - cl.inset[3]);
        let need: Vec<(f32, f32)> = regions
            .iter()
            .map(|(r, l)| {
                let ri = &self.g.clusters[*r];
                ((l.w + ri.inset[0] + ri.inset[2]).max(ri.min.0), (l.h + ri.inset[1] + ri.inset[3]).max(ri.min.1))
            })
            .collect();
        let used: f32 = need.iter().map(|n| if vertical { n.1 } else { n.0 }).sum();
        let extra = ((if vertical { ah } else { aw }) - used).max(0.0) / regions.len().max(1) as f32;
        let mut at = if vertical { ay } else { ax };
        for ((r, local), (nw, nh)) in regions.into_iter().zip(need) {
            let rect = if vertical { Rect::new(ax, at, aw, nh + extra) } else { Rect::new(at, ay, nw + extra, ah) };
            at += if vertical { rect.h } else { rect.w };
            out.clusters.push((r, rect));
            // The members are centered in their region.
            let inner = self.g.clusters[r].inset;
            let dx = rect.x + inner[0] + ((rect.w - inner[0] - inner[2]) - local.w) / 2.0;
            let dy = rect.y + inner[1] + ((rect.h - inner[1] - inner[3]) - local.h) / 2.0;
            out.absorb(local, dx, dy);
        }
    }

    fn solve(&self, scope: usize) -> Result<Local, String> {
        let g = self.g;
        // Concurrent stacks that live in this scope: each region first, then the stack
        // becomes one box.
        let mut stacks: Vec<(usize, Vec<(usize, Local)>)> = Vec::new();
        for s in (0..g.clusters.len()).filter(|&s| self.cscope[s] == scope && self.is_stack(s)) {
            let mut regions = Vec::new();
            for r in (0..g.clusters.len()).filter(|&r| g.clusters[r].parent == Some(s)) {
                regions.push((r, self.solve(r + 1)?));
            }
            stacks.push((s, regions));
        }

        let mut dg = DotGraph::default();
        let mut cmap: BTreeMap<usize, usize> = BTreeMap::new();
        for c in (0..g.clusters.len()).filter(|&c| self.cscope[c] == scope && !self.is_stack(c) && !self.is_region(c)) {
            cmap.insert(c, dg.clusters.len());
            dg.clusters.push(DotCluster { id: c, parent: None });
        }
        for (&c, &k) in &cmap {
            let parent = self.dot_parent(g.clusters[c].parent, scope).and_then(|p| cmap.get(&p).copied());
            dg.clusters[k].parent = parent;
        }
        let parent_of = |p: Option<usize>| self.dot_parent(p, scope).and_then(|c| cmap.get(&c).copied());
        let mut nmap: BTreeMap<usize, usize> = BTreeMap::new();
        for (i, n) in g.nodes.iter().enumerate().filter(|(i, _)| self.nscope[*i] == scope) {
            nmap.insert(i, dg.nodes.len());
            dg.nodes.push(DotNode { w: n.w, h: n.h, border: n.border, parent: parent_of(n.parent), origin: Origin::Node(i) });
        }
        let mut bmap: BTreeMap<usize, usize> = BTreeMap::new();
        let mut sizes: BTreeMap<usize, (f32, f32)> = BTreeMap::new();
        for (s, regions) in &stacks {
            let (w, h) = self.block_size(*s, regions);
            sizes.insert(*s, (w, h));
            bmap.insert(*s, dg.nodes.len());
            dg.nodes.push(DotNode { w, h, border: Border::Rect, parent: parent_of(g.clusters[*s].parent), origin: Origin::Block(*s) });
        }
        // Empty clusters get a node to hang their box on.
        let mut filled = vec![false; dg.clusters.len()];
        for n in &dg.nodes {
            let mut c = n.parent;
            while let Some(k) = c {
                filled[k] = true;
                c = dg.clusters[k].parent;
            }
        }
        for k in (0..filled.len()).filter(|&k| !filled[k]) {
            let cl = &g.clusters[dg.clusters[k].id];
            let (w, h) = ((cl.min.0 - cl.inset[0] - cl.inset[2]).max(1.0), (cl.min.1 - cl.inset[1] - cl.inset[3]).max(1.0));
            dg.nodes.push(DotNode { w, h, border: Border::Rect, parent: Some(k), origin: Origin::Phantom });
        }
        // The node an edge to a cluster is drawn to: its first member.
        let rep = |dg: &DotGraph, k: usize| (0..dg.nodes.len()).find(|&n| dg.node_within(n, k));

        for (ei, e) in g.edges.iter().enumerate() {
            let l = &self.lifts[ei];
            if l.scope != scope || l.across {
                continue;
            }
            let resolve = |dg: &DotGraph, end: End| -> Option<(usize, Option<usize>)> {
                match end {
                    End::Node(i) => nmap.get(&i).map(|&n| (n, None)),
                    End::Cluster(c) if bmap.contains_key(&c) => Some((bmap[&c], None)),
                    End::Cluster(c) => {
                        let k = *cmap.get(&c)?;
                        rep(dg, k).map(|n| (n, Some(k)))
                    }
                }
            };
            let (Some((a, ca)), Some((b, cb))) = (resolve(&dg, l.from), resolve(&dg, l.to)) else { continue };
            // A line from a cluster to something inside it has nothing to be clipped at.
            let ca = ca.filter(|&k| !dg.node_within(b, k));
            let cb = cb.filter(|&k| !dg.node_within(a, k));
            let (len, flipped, horizontal) = match e.hint {
                Hint::Auto(n) => (n.max(1), false, n <= 1),
                // Dot's ranks run the other way when reversed, so the screen directions swap.
                Hint::Dir(Dir::Down) => (2, self.o.dir.reverse(), false),
                Hint::Dir(Dir::Up) => (2, !self.o.dir.reverse(), false),
                Hint::Dir(Dir::Right) => (1, false, true),
                Hint::Dir(Dir::Left) => (1, true, true),
            };
            let mut de = DotEdge { id: ei, tail: a, head: b, ltail: ca, lhead: cb, minlen: len - 1, flipped, label: e.label, tail_label: e.tail_label, head_label: e.head_label };
            if flipped {
                std::mem::swap(&mut de.tail, &mut de.head);
                std::mem::swap(&mut de.ltail, &mut de.lhead);
                std::mem::swap(&mut de.tail_label, &mut de.head_label);
            }
            // Up and left arrows keep a rank between their ends; only the sideways ones
            // share one.
            if horizontal && a != b && dg.nodes[a].parent == dg.nodes[b].parent {
                dg.same.push((de.tail, de.head));
            }
            dg.edges.push(de);
        }

        let placed = self.run_dot(&dg)?;
        let mut local = Local { w: placed.size.0 as f32, h: placed.size.1 as f32, ..Local::default() };
        for n in &placed.nodes {
            let Some(dn) = dg.nodes.get(n.id) else { continue };
            let rect = Rect::new((n.x - n.w / 2.0) as f32, (n.y - n.h / 2.0) as f32, n.w as f32, n.h as f32);
            match dn.origin {
                Origin::Node(i) => local.nodes.push((i, rect)),
                Origin::Block(s) => {
                    local.clusters.push((s, rect));
                    if let Some(pos) = stacks.iter().position(|(x, _)| *x == s) {
                        let regions = std::mem::take(&mut stacks[pos].1);
                        self.place_regions(s, rect, regions, &mut local);
                    }
                }
                Origin::Phantom => {}
            }
        }
        for c in &placed.clusters {
            let Some(dc) = dg.clusters.get(c.id) else { continue };
            let [x0, y0, x1, y1] = c.rect;
            local.clusters.push((dc.id, Rect::new(x0 as f32, y0 as f32, (x1 - x0) as f32, (y1 - y0) as f32)));
        }
        // Dot reports edges by the caller's index, which `comment` carries.
        let mut by_id: Vec<Option<usize>> = vec![None; g.edges.len()];
        for (k, de) in dg.edges.iter().enumerate() {
            by_id[de.id] = Some(k);
        }
        for e in &placed.edges {
            let Some(de) = by_id.get(e.id).copied().flatten().map(|k| &dg.edges[k]) else { continue };
            let pt = |p: Option<(f64, f64)>| p.map(|(x, y)| (x as f32, y as f32));
            let mut segs = Vec::new();
            for curve in &e.curves {
                for s in curve.windows(4).step_by(3) {
                    segs.push([(s[0].0 as f32, s[0].1 as f32), (s[1].0 as f32, s[1].1 as f32), (s[2].0 as f32, s[2].1 as f32), (s[3].0 as f32, s[3].1 as f32)]);
                }
            }
            let mut route = Route { segs, label: pt(e.label), tail_label: pt(e.tail_label), head_label: pt(e.head_label) };
            if de.flipped {
                route = route.reversed();
            }
            local.routes.push((de.id, route));
        }
        Ok(local)
    }

    /// Runs dot; if it chokes on cluster-to-cluster edges or same-rank constraints
    /// (it does for some combinations), tries again without them.
    fn run_dot(&self, dg: &DotGraph) -> Result<Placed, String> {
        if dg.nodes.is_empty() {
            return Ok(Placed::default());
        }
        match graphviz::layout(&dot_text(dg, &self.g.clusters, self.o, true)) {
            Ok(p) => Ok(p),
            Err(_) => graphviz::layout(&dot_text(dg, &self.g.clusters, self.o, false)),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Side {
    Top,
    Bottom,
    Left,
    Right,
}

impl Side {
    fn outward(self) -> Pt {
        match self {
            Side::Top => (0.0, -1.0),
            Side::Bottom => (0.0, 1.0),
            Side::Left => (-1.0, 0.0),
            Side::Right => (1.0, 0.0),
        }
    }

    fn horizontal(self) -> bool {
        matches!(self, Side::Top | Side::Bottom)
    }

    /// The side of `r` a line ending at `p`, arriving from `c`, meets. At a corner the
    /// side the line comes from wins.
    fn meeting(r: &Rect, p: Pt, c: Pt) -> Side {
        let sides = [
            (Side::Top, (p.1 - r.y).abs(), r.y - c.1),
            (Side::Bottom, (p.1 - r.bottom()).abs(), c.1 - r.bottom()),
            (Side::Left, (p.0 - r.x).abs(), r.x - c.0),
            (Side::Right, (p.0 - r.right()).abs(), c.0 - r.right()),
        ];
        let nearest = sides.iter().map(|s| s.1).fold(f32::MAX, f32::min);
        sides.iter().filter(|s| s.1 <= nearest + 1.5).max_by(|a, b| a.2.total_cmp(&b.2)).map_or(Side::Top, |s| s.0)
    }
}

/// The end of a route and the control point next to it.
fn route_end(r: &Route, start: bool) -> Option<(Pt, Pt)> {
    let s = if start { r.segs.first()? } else { r.segs.last()? };
    let (p, c) = if start { (s[0], s[1]) } else { (s[3], s[2]) };
    // A control point sitting on the end says nothing about the direction.
    Some((p, if dist(p, c) < 1e-3 { if start { s[2] } else { s[1] } } else { c }))
}

fn set_route_end(r: &mut Route, start: bool, p: Pt, c: Option<Pt>) {
    if start {
        let s = &mut r.segs[0];
        s[0] = p;
        s[1] = c.unwrap_or(s[1]);
    } else {
        let n = r.segs.len() - 1;
        let s = &mut r.segs[n];
        s[3] = p;
        s[2] = c.unwrap_or(s[2]);
    }
}

/// How far from a corner a line end is kept at the least.
const CORNER: f32 = 8.0;

/// Distance a line end keeps from a node's corners; slanted outlines need more.
fn corner_of(b: Border) -> f32 {
    match b {
        Border::Slanted(k) => (k as f32).max(CORNER),
        _ => CORNER,
    }
}

/// Dot ends a spline wherever it happens to cross the node's outline, which for a line
/// sliding along the bottom of a box is its corner, on top of the other lines that
/// leave from there. Such ends are moved onto the side of the box that faces the other
/// end and spread along it; icons get their ends on the figure instead of the box.
fn settle_ends(out: &mut [Route], g: &Graph, nodes: &[Rect], lifts: &[Lift]) {
    const SPACING: f32 = 14.0;
    // (route, start?, node, side, wanted position along the side)
    let mut moves: Vec<(usize, bool, usize, Side, f32, bool)> = Vec::new();
    let mut occupied: BTreeMap<(usize, Side), Vec<f32>> = BTreeMap::new();
    for (i, e) in g.edges.iter().enumerate() {
        if lifts[i].across || e.from == e.to || out[i].segs.is_empty() {
            continue;
        }
        for (start, own, lifted) in [(true, e.from, lifts[i].from != e.from), (false, e.to, lifts[i].to != e.to)] {
            let End::Node(n) = own else { continue };
            let Some((p, c)) = route_end(&out[i], start) else { continue };
            let r = nodes[n];
            match g.nodes[n].border {
                Border::Icon(..) if !lifted => {
                    let q = clip(&r, g.nodes[n].border, c);
                    set_route_end(&mut out[i], start, q, None);
                }
                Border::Rect | Border::Slanted(_) if !lifted => {
                    let corner = corner_of(g.nodes[n].border);
                    let side = Side::meeting(&r, p, c);
                    let (lo, hi, at, line, across) = if side.horizontal() { (r.x, r.right(), p.0, if side == Side::Top { r.y } else { r.bottom() }, p.1) } else { (r.y, r.bottom(), p.1, if side == Side::Left { r.x } else { r.right() }, p.0) };
                    let n_out = side.outward();
                    let away = unit_vec(p, c);
                    let cos = (away.0 * n_out.0 + away.1 * n_out.1).abs();
                    let settled = (across - line).abs() < 1.5 && at > lo + corner && at < hi - corner && cos > 0.25;
                    if settled {
                        occupied.entry((n, side)).or_default().push(at);
                    } else {
                        // The nearest spot on the side keeps the rest of the curve as dot drew it.
                        moves.push((i, start, n, side, at.clamp(lo + corner, (hi - corner).max(lo + corner)), cos > 0.25));
                    }
                }
                _ => {}
            }
        }
    }
    moves.sort_by(|a, b| (a.2, a.3).cmp(&(b.2, b.3)).then(a.4.total_cmp(&b.4)));
    for (i, start, n, side, want, steep) in moves {
        let r = nodes[n];
        let corner = corner_of(g.nodes[n].border);
        let (lo, hi) = if side.horizontal() { (r.x + corner, r.right() - corner) } else { (r.y + corner, r.bottom() - corner) };
        let used = occupied.entry((n, side)).or_default();
        let free = |x: f32| used.iter().all(|u| (u - x).abs() >= SPACING);
        let at = (0..8)
            .flat_map(|k| [1.0f32, -1.0].map(|sgn| (want + sgn * SPACING * k as f32 / 2.0).clamp(lo, hi.max(lo))))
            .find(|&x| free(x))
            .unwrap_or(want);
        used.push(at);
        let q = if side.horizontal() { (at, if side == Side::Top { r.y } else { r.bottom() }) } else { (if side == Side::Left { r.x } else { r.right() }, at) };
        let seg = if start { out[i].segs[0] } else { out[i].segs[out[i].segs.len() - 1] };
        let o = side.outward();
        // A line that already meets the side head on only slides; a shallow one is
        // turned to meet it squarely.
        let ctrl = match route_end(&out[i], start) {
            Some((p, c)) if steep => (c.0 + q.0 - p.0, c.1 + q.1 - p.1),
            _ => {
                let reach = (dist(seg[0], seg[3]) * 0.4).clamp(8.0, 30.0);
                (q.0 + o.0 * reach, q.1 + o.1 * reach)
            }
        };
        set_route_end(&mut out[i], start, q, Some(ctrl));
    }
}

fn unit_vec(a: Pt, b: Pt) -> Pt {
    let d = dist(a, b).max(1e-3);
    ((b.0 - a.0) / d, (b.1 - a.1) / d)
}

pub fn layout(g: &Graph, o: &Opts) -> Result<Layout, String> {
    let solver = Solver::new(g, o);
    let root = solver.solve(0)?;
    let mut nodes: Vec<Option<Rect>> = vec![None; g.nodes.len()];
    let mut clusters: Vec<Option<Rect>> = vec![None; g.clusters.len()];
    let mut routes: Vec<Option<Route>> = vec![None; g.edges.len()];
    let (w, h) = (root.w, root.h);
    for (i, r) in root.nodes {
        nodes[i] = Some(r);
    }
    for (i, r) in root.clusters {
        clusters[i] = Some(r);
    }
    for (i, r) in root.routes {
        routes[i] = Some(r);
    }
    let nodes: Vec<Rect> = nodes.into_iter().map(|r| r.unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0))).collect();
    let clusters: Vec<Rect> = clusters.into_iter().map(|r| r.unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0))).collect();

    let rect_of = |e: End| match e {
        End::Node(i) => (nodes[i], g.nodes[i].border),
        End::Cluster(c) => (clusters[c], Border::Rect),
    };
    let mut out: Vec<Route> = Vec::with_capacity(g.edges.len());
    for (ei, e) in g.edges.iter().enumerate() {
        let lift = &solver.lifts[ei];
        let (fr, fb) = rect_of(e.from);
        let (tr, tb) = rect_of(e.to);
        let route = match routes[ei].take() {
            // Ends that were moved up to a stack's box continue in a straight line to
            // the real target.
            Some(mut r) if !lift.across => {
                if lift.from != e.from {
                    let p = r.start();
                    let q = clip(&fr, fb, p);
                    r.segs.insert(0, Route::line(q, p).segs[0]);
                }
                if lift.to != e.to {
                    let p = r.end();
                    let q = clip(&tr, tb, p);
                    r.segs.push(Route::line(p, q).segs[0]);
                }
                r
            }
            _ => {
                let (a, b) = (clip(&fr, fb, tr.center()), clip(&tr, tb, fr.center()));
                Route::line(a, b)
            }
        };
        out.push(route);
    }

    settle_ends(&mut out, g, &nodes, &solver.lifts);

    // Dot parks a label at its virtual node, but the spline only has to stay left of it
    // and may drift away from that spot, and an edge clipped at a cluster border can
    // start somewhere else than the label expects. A label that is not next to its own
    // line is moved to a spot beside it that nothing else occupies.
    let polys: Vec<Vec<Pt>> = out.iter().map(Route::flatten).collect();
    let mut others: Vec<Option<Rect>> = g.edges.iter().zip(&out).map(|(e, r)| r.label.zip(e.label).map(|(c, (w, h))| rect_at(c, w, h))).collect();
    for (i, e) in g.edges.iter().enumerate() {
        let (Some(c), Some((w, h))) = (out[i].label, e.label) else { continue };
        let beside_own = |at: Pt| (1.5..=LABEL_GAP * 3.0).contains(&rect_gap(&rect_at(at, w, h), &polys[i]));
        let nothing_in_the_way = |at: Pt| !nodes.iter().any(|n| rects_hit(n, &rect_at(at, w, h)));
        let free = |at: Pt| {
            let r = rect_at(at, w, h);
            let probe = Rect::new(r.x + 0.5, r.y + 0.5, r.w - 1.0, r.h - 1.0);
            beside_own(at) && nothing_in_the_way(at) && !others.iter().enumerate().any(|(j, o)| j != i && o.is_some_and(|o| rects_hit(&o, &probe))) && !polys.iter().enumerate().any(|(j, p)| j != i && hits_polys(&probe, std::slice::from_ref(p)) > 0)
        };
        if free(c) {
            continue;
        }
        let snapped = snap_to_route(&polys[i], c, (w, h), o.dir.lr()).into_iter();
        let beside = [0.5f32, 0.35, 0.65, 0.2, 0.8].into_iter().flat_map(|t| {
            let (m, off) = beside_at(&polys[i], t, |t| t, (w, h));
            [-1.0f32, 1.0].map(|k| (m.0 + off.0 * k, m.1 + off.1 * k))
        });
        // Crossing a neighbour's line is better than sitting on one's own.
        let spots: Vec<Pt> = snapped.chain(beside).collect();
        if let Some(at) = spots.iter().copied().find(|&at| free(at)).or_else(|| spots.iter().copied().find(|&at| beside_own(at) && nothing_in_the_way(at))) {
            out[i].label = Some(at);
            others[i] = Some(rect_at(at, w, h));
        }
    }

    // Dot keeps end labels off nodes and their own line only. One that sits on any line,
    // or on a label placed before it, gets a new spot near the end of its edge. The pass
    // above only moved labels, not routes, so `polys` is still valid here.
    let mut taken: Vec<Rect> = Vec::new();
    for (e, r) in g.edges.iter().zip(&out) {
        if let (Some(c), Some((w, h))) = (r.label, e.label) {
            taken.push(rect_at(c, w, h));
        }
    }
    for (i, e) in g.edges.iter().enumerate() {
        for at_head in [false, true] {
            let (spot, size) = if at_head { (out[i].head_label, e.head_label) } else { (out[i].tail_label, e.tail_label) };
            let Some((w, h)) = size else { continue };
            let clear = |c: Pt| {
                let r = rect_at(c, w, h);
                let probe = Rect::new(r.x + 1.0, r.y + 1.0, r.w - 2.0, r.h - 2.0);
                // The head or tail decoration (diamond, circle) sits on the line's end.
                let end = if at_head { out[i].end() } else { out[i].start() };
                let off_the_end = dist(end, (end.0.clamp(r.x, r.right()), end.1.clamp(r.y, r.bottom()))) >= END_CLEARANCE;
                off_the_end && !nodes.iter().any(|n| rects_hit(n, &probe)) && !taken.iter().any(|t| rects_hit(t, &probe)) && hits_polys(&probe, &polys) == 0
            };
            let found = spot.filter(|&c| clear(c)).or_else(|| {
                let poly = &polys[i];
                let total: f32 = poly.windows(2).map(|w| dist(w[0], w[1])).sum::<f32>().max(1.0);
                let toward_end = |t: f32| if at_head { 1.0 - t } else { t };
                // Close to the node first, on both sides of the line.
                [6.0f32, 14.0, 24.0, 36.0, 52.0].into_iter().find_map(|along| {
                    let t = (along + (if o.dir.lr() { w } else { h }) / 2.0) / total;
                    let (m, off) = beside_at(poly, t, toward_end, (w, h));
                    [1.0f32, -1.0].into_iter().map(|k| (m.0 + off.0 * k, m.1 + off.1 * k)).find(|&c| clear(c))
                })
            });
            if at_head {
                out[i].head_label = found.or(spot);
            } else {
                out[i].tail_label = found.or(spot);
            }
            if let Some(c) = found.or(spot) {
                taken.push(rect_at(c, w, h));
            }
        }
    }

    let mut obstacles = nodes.clone();
    for (e, r) in g.edges.iter().zip(&out) {
        for (c, s) in [(r.label, e.label), (r.tail_label, e.tail_label), (r.head_label, e.head_label)] {
            if let (Some(c), Some((w, h))) = (c, s) {
                obstacles.push(rect_at(c, w, h));
            }
        }
    }
    Ok(Layout { nodes, clusters, routes: out, w, h, obstacles, polys })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chain() -> Graph {
        let node = Node { w: 60.0, h: 30.0, parent: None, border: Border::Rect };
        let edge = Edge { from: End::Node(0), to: End::Node(1), hint: Hint::Auto(2), label: None, tail_label: None, head_label: None };
        Graph { nodes: vec![node.clone(), node], clusters: vec![], edges: vec![edge] }
    }

    #[test]
    fn dot_gets_the_rank_direction() {
        let dg = DotGraph::default();
        for (dir, want) in [(RankDir::TB, "TB"), (RankDir::BT, "BT"), (RankDir::LR, "LR"), (RankDir::RL, "RL")] {
            let text = dot_text(&dg, &[], &Opts { dir, ..Opts::default() }, false);
            assert!(text.contains(&format!("rankdir={want} ")), "{text}");
        }
    }

    #[test]
    fn reverse_puts_the_head_before_the_tail() {
        let g = chain();
        let at = |dir| {
            let l = layout(&g, &Opts { dir, ..Opts::default() }).unwrap();
            (l.nodes[0].center(), l.nodes[1].center())
        };
        let (a, b) = at(RankDir::TB);
        assert!(b.1 > a.1, "{a:?} {b:?}");
        let (a, b) = at(RankDir::BT);
        assert!(b.1 < a.1, "{a:?} {b:?}");
        let (a, b) = at(RankDir::LR);
        assert!(b.0 > a.0, "{a:?} {b:?}");
        let (a, b) = at(RankDir::RL);
        assert!(b.0 < a.0, "{a:?} {b:?}");
    }

    #[test]
    fn directed_hints_keep_their_screen_direction_when_reversed() {
        let mut g = chain();
        g.edges[0].hint = Hint::Dir(Dir::Down);
        let l = layout(&g, &Opts { dir: RankDir::BT, ..Opts::default() }).unwrap();
        assert!(l.nodes[1].center().1 > l.nodes[0].center().1);
    }

    #[test]
    fn slanted_nodes_keep_line_ends_off_the_leaning_corners() {
        let end_x = |border| {
            let g = Graph {
                nodes: vec![Node { w: 100.0, h: 40.0, parent: None, border }, Node { w: 100.0, h: 40.0, parent: None, border: Border::Rect }],
                clusters: vec![],
                edges: vec![Edge { from: End::Node(1), to: End::Node(0), hint: Hint::Auto(2), label: None, tail_label: None, head_label: None }],
            };
            let nodes = [Rect::new(0.0, 0.0, 100.0, 40.0), Rect::new(0.0, 140.0, 100.0, 40.0)];
            let lifts = [Lift { scope: 0, from: End::Node(1), to: End::Node(0), across: false }];
            // Arrives at the bottom side, 3 pt from its left corner.
            let mut routes = vec![Route::line((50.0, 140.0), (3.0, 40.0))];
            settle_ends(&mut routes, &g, &nodes, &lifts);
            routes[0].end().0
        };
        assert_eq!(end_x(Border::Rect), CORNER);
        assert_eq!(end_x(Border::Slanted(30)), 30.0);
        assert_eq!(end_x(Border::Slanted(2)), CORNER);
    }

    #[test]
    fn slanted_nodes_are_clipped_like_boxes() {
        let r = Rect::new(0.0, 0.0, 100.0, 40.0);
        assert_eq!(clip(&r, Border::Slanted(12), (50.0, 200.0)), clip(&r, Border::Rect, (50.0, 200.0)));
    }
}
