//! Edges: terminal attachment, routing for each draw.io edge style, the line itself
//! (rounded or curved) and the arrow markers.

use std::borrow::Cow;
use std::f32::consts::PI;

use crate::scene::{Cap, Join, PathBuilder, Scene, Stroke, Style as SceneStyle};

use super::geometry::{rotate_about, Pt, Rect};
use super::shapes::{self, Outline};
use super::style::Style;

// Terminals -----------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Terminal<'a> {
    pub rect: Rect,
    pub style: &'a Style,
    pub outline: Outline,
}

impl<'a> Terminal<'a> {
    pub fn new(style: &'a Style, rect: Rect) -> Terminal<'a> {
        Terminal { rect, style, outline: shapes::outline(style, rect) }
    }

    fn grown(&self, d: f32) -> Cow<'_, Terminal<'a>> {
        if d.abs() < 1e-3 {
            return Cow::Borrowed(self);
        }
        Cow::Owned(Terminal::new(self.style, self.rect.grow(d)))
    }

    fn rot(&self) -> f32 {
        self.style.num("rotation", 0.0)
    }
}

pub struct EdgeSpec<'a> {
    pub style: &'a Style,
    pub source: Option<&'a Terminal<'a>>,
    pub target: Option<&'a Terminal<'a>>,
    pub waypoints: &'a [Pt],
    pub source_point: Option<Pt>,
    pub target_point: Option<Pt>,
    /// Source and target are the same cell.
    pub self_loop: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Dir {
    N,
    E,
    S,
    W,
}

impl Dir {
    const ALL: [Dir; 4] = [Dir::E, Dir::S, Dir::W, Dir::N];

    fn vec(self) -> Pt {
        match self {
            Dir::N => (0.0, -1.0),
            Dir::E => (1.0, 0.0),
            Dir::S => (0.0, 1.0),
            Dir::W => (-1.0, 0.0),
        }
    }

    fn horizontal(self) -> bool {
        matches!(self, Dir::E | Dir::W)
    }

    fn turned(self, quarters: i32) -> Dir {
        let i = Dir::ALL.iter().position(|d| *d == self).unwrap() as i32;
        Dir::ALL[(i + quarters).rem_euclid(4) as usize]
    }
}

fn point_rect(p: Pt) -> Rect {
    Rect::new(p.0, p.1, 0.0, 0.0)
}

fn dist(a: Pt, b: Pt) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

/// A fixed attachment from `exitX/exitY/exitDx/exitDy` (or the `entry` variants).
struct Fixed {
    pt: Pt,
    dir: Option<Dir>,
}

fn fixed_point(style: &Style, prefix: &str, term: &Terminal) -> Option<Fixed> {
    let xk = format!("{prefix}X");
    let yk = format!("{prefix}Y");
    if !style.has(&xk) && !style.has(&yk) {
        return None;
    }
    let x = style.num(&xk, 0.5);
    let y = style.num(&yk, 0.5);
    let dx = style.num(&format!("{prefix}Dx"), 0.0);
    let dy = style.num(&format!("{prefix}Dy"), 0.0);
    let r = term.rect;
    let raw = (r.x + x * r.w, r.y + y * r.h);
    let mut pt = (raw.0 + dx, raw.1 + dy);
    if style.flag(&format!("{prefix}Perimeter"), true)
        && dx == 0.0
        && dy == 0.0
        && (raw.0 - r.cx()).abs() + (raw.1 - r.cy()).abs() > 1e-3
    {
        pt = term.outline.hit_ray(r.center(), raw);
    }
    let rot = term.rot();
    pt = rotate_about(pt, r.center(), rot);
    let dir = if x <= 0.0 {
        Some(Dir::W)
    } else if x >= 1.0 {
        Some(Dir::E)
    } else if y <= 0.0 {
        Some(Dir::N)
    } else if y >= 1.0 {
        Some(Dir::S)
    } else {
        None
    };
    let quarters = (rot / 90.0).round() as i32;
    Some(Fixed { pt, dir: dir.map(|d| d.turned(quarters)) })
}

// Routing -------------------------------------------------------------------------------

/// Absolute route points from the source end to the target end. Empty when the edge
/// has nothing to attach to.
pub fn route(spec: &EdgeSpec, warnings: &mut Vec<String>) -> Vec<Pt> {
    let style = spec.style;
    let edge_style = style.get("edgeStyle").unwrap_or("");

    // Perimeter spacing grows the terminal the edge attaches to.
    let spacing = style.num("perimeterSpacing", 0.0);
    let s_term = spec.source.map(|t| t.grown(spacing + style.num("sourcePerimeterSpacing", 0.0)));
    let t_term = spec.target.map(|t| t.grown(spacing + style.num("targetPerimeterSpacing", 0.0)));
    let spec = Resolved {
        style,
        source: s_term.as_deref(),
        target: t_term.as_deref(),
        waypoints: spec.waypoints,
        sp: spec.source_point,
        tp: spec.target_point,
        self_loop: spec.self_loop,
    };

    if spec.source.is_none() && spec.target.is_none() && spec.sp.is_none() && spec.tp.is_none() {
        return Vec::new();
    }

    match edge_style {
        "" | "none" => straight(&spec),
        "orthogonalEdgeStyle" | "segmentEdgeStyle" => orthogonal(&spec),
        "elbowEdgeStyle" => elbow(&spec, style.get("elbow") == Some("vertical")),
        "sideToSide" => elbow(&spec, false),
        "topToBottom" => elbow(&spec, true),
        "entityRelationEdgeStyle" => entity_relation(&spec),
        "loopEdgeStyle" => straight(&spec),
        "isometricEdgeStyle" => {
            warnings.push("isometricEdgeStyle is not supported; the edge is routed orthogonally".into());
            orthogonal(&spec)
        }
        other => {
            warnings.push(format!("edge style \"{other}\" is not supported; the edge is drawn as a straight line"));
            straight(&spec)
        }
    }
}

struct Resolved<'a> {
    style: &'a Style,
    source: Option<&'a Terminal<'a>>,
    target: Option<&'a Terminal<'a>>,
    waypoints: &'a [Pt],
    sp: Option<Pt>,
    tp: Option<Pt>,
    self_loop: bool,
}

impl Resolved<'_> {
    fn s_fixed(&self) -> Option<Fixed> {
        self.source.and_then(|t| fixed_point(self.style, "exit", t))
    }

    fn t_fixed(&self) -> Option<Fixed> {
        self.target.and_then(|t| fixed_point(self.style, "entry", t))
    }

    fn s_box(&self) -> Rect {
        match (self.source, self.sp) {
            (Some(t), _) => t.rect,
            (None, Some(p)) => point_rect(p),
            _ => point_rect(self.waypoints.first().copied().or(self.tp).unwrap_or((0.0, 0.0))),
        }
    }

    fn t_box(&self) -> Rect {
        match (self.target, self.tp) {
            (Some(t), _) => t.rect,
            (None, Some(p)) => point_rect(p),
            _ => point_rect(self.waypoints.last().copied().or(self.sp).unwrap_or((0.0, 0.0))),
        }
    }

    fn is_loop(&self) -> bool {
        self.self_loop && self.source.is_some() && self.waypoints.is_empty()
    }
}

fn loop_route(r: &Resolved) -> Vec<Pt> {
    let b = r.s_box();
    let out = 30.0;
    vec![
        (b.right(), b.cy() - b.h / 6.0),
        (b.right() + out, b.cy() - b.h / 6.0),
        (b.right() + out, b.cy() + b.h / 6.0),
        (b.right(), b.cy() + b.h / 6.0),
    ]
}

fn straight(r: &Resolved) -> Vec<Pt> {
    if r.is_loop() && r.s_fixed().is_none() && r.t_fixed().is_none() {
        return loop_route(r);
    }
    let s_fixed = r.s_fixed();
    let t_fixed = r.t_fixed();
    let t_center = r.target.map(|t| t.rect.center()).or(r.tp);
    let s_center = r.source.map(|t| t.rect.center()).or(r.sp);

    let next_for_source = r.waypoints.first().copied().or(t_fixed.as_ref().map(|f| f.pt)).or(t_center).or(r.sp);
    let s = match (&s_fixed, r.source) {
        (Some(f), _) => f.pt,
        (None, Some(t)) => match next_for_source {
            Some(n) => t.outline.hit_ray(t.rect.center(), n),
            None => t.rect.center(),
        },
        (None, None) => r.sp.or_else(|| r.waypoints.first().copied()).unwrap_or((0.0, 0.0)),
    };
    let next_for_target = r.waypoints.last().copied().or(Some(s)).or(s_center);
    let t = match (&t_fixed, r.target) {
        (Some(f), _) => f.pt,
        (None, Some(t)) => match next_for_target {
            Some(n) => t.outline.hit_ray(t.rect.center(), n),
            None => t.rect.center(),
        },
        (None, None) => r.tp.or_else(|| r.waypoints.last().copied()).unwrap_or(s),
    };
    // A source that was floating toward the target center must aim at where the
    // target edge actually is for a clean line when both ends float.
    let mut pts = vec![s];
    pts.extend_from_slice(r.waypoints);
    pts.push(t);
    pts
}

/// Both ends of an orthogonal route: the attachment point, the direction it leaves
/// in and whether the point is fixed by the style.
struct End {
    pt: Pt,
    dir: Dir,
    boxr: Rect,
    fixed: bool,
}

/// The side of `from` that faces `toward`, preferring the axis with the bigger gap.
fn facing(from: Rect, toward: Rect) -> Dir {
    let hgap = if toward.x >= from.right() {
        toward.x - from.right()
    } else if toward.right() <= from.x {
        from.x - toward.right()
    } else {
        -1.0
    };
    let vgap = if toward.y >= from.bottom() {
        toward.y - from.bottom()
    } else if toward.bottom() <= from.y {
        from.y - toward.bottom()
    } else {
        -1.0
    };
    let (dx, dy) = (toward.cx() - from.cx(), toward.cy() - from.cy());
    let horizontal = match (hgap >= 0.0, vgap >= 0.0) {
        (true, false) => true,
        (false, true) => false,
        (true, true) => hgap >= vgap,
        (false, false) => dx.abs() >= dy.abs(),
    };
    match (horizontal, if horizontal { dx >= 0.0 } else { dy >= 0.0 }) {
        (true, true) => Dir::E,
        (true, false) => Dir::W,
        (false, true) => Dir::S,
        (false, false) => Dir::N,
    }
}

/// Point where a line at `coord` (y for E/W, x for N/S) leaves the shape through the
/// side `dir`.
fn side_point(term: Option<&Terminal>, fallback: Rect, dir: Dir, coord: Option<f32>) -> Pt {
    let r = term.map_or(fallback, |t| t.rect);
    let c = coord.unwrap_or(if dir.horizontal() { r.cy() } else { r.cx() });
    let Some(t) = term else { return (r.x, r.y) };
    match dir {
        Dir::E | Dir::W => match t.outline.hline(c) {
            Some((x0, x1)) => (if dir == Dir::E { x1 } else { x0 }, c),
            None => (if dir == Dir::E { r.right() } else { r.x }, c),
        },
        Dir::N | Dir::S => match t.outline.vline(c) {
            Some((y0, y1)) => (c, if dir == Dir::S { y1 } else { y0 }),
            None => (c, if dir == Dir::S { r.bottom() } else { r.y }),
        },
    }
}

const JETTY: f32 = 20.0;
const MIN_LEG: f32 = 1.0;

fn orthogonal(r: &Resolved) -> Vec<Pt> {
    if r.is_loop() && r.s_fixed().is_none() && r.t_fixed().is_none() {
        return loop_route(r);
    }
    let s_fixed = r.s_fixed();
    let t_fixed = r.t_fixed();
    let (sb, tb) = (r.s_box(), r.t_box());
    let hints = r.waypoints;

    let s_ref = hints.first().map(|&p| point_rect(p)).or(t_fixed.as_ref().map(|f| point_rect(f.pt))).unwrap_or(tb);
    let t_ref = hints.last().map(|&p| point_rect(p)).or(s_fixed.as_ref().map(|f| point_rect(f.pt))).unwrap_or(sb);

    let s_dir = s_fixed.as_ref().and_then(|f| f.dir);
    let t_dir = t_fixed.as_ref().and_then(|f| f.dir);
    let s_options: Vec<Dir> = match s_dir {
        Some(d) => vec![d],
        None if s_fixed.is_some() || t_dir.is_some() => {
            let mut v = vec![facing(sb, s_ref)];
            v.extend(Dir::ALL);
            v
        }
        None => vec![facing(sb, s_ref)],
    };
    let t_options: Vec<Dir> = match t_dir {
        Some(d) => vec![d],
        None if t_fixed.is_some() || s_dir.is_some() => {
            let mut v = vec![facing(tb, t_ref)];
            v.extend(Dir::ALL);
            v
        }
        None => vec![facing(tb, t_ref)],
    };

    let mut best: Option<(f32, Vec<Pt>)> = None;
    for &ds in &s_options {
        for &dt in &t_options {
            let mut s = End {
                pt: s_fixed.as_ref().map_or_else(|| side_point(r.source, sb, ds, None), |f| f.pt),
                dir: ds,
                boxr: sb,
                fixed: s_fixed.is_some(),
            };
            let mut t = End {
                pt: t_fixed.as_ref().map_or_else(|| side_point(r.target, tb, dt, None), |f| f.pt),
                dir: dt,
                boxr: tb,
                fixed: t_fixed.is_some(),
            };
            if r.source.is_none() {
                s.pt = r.sp.unwrap_or(s.pt);
            }
            if r.target.is_none() {
                t.pt = r.tp.unwrap_or(t.pt);
            }
            if hints.is_empty() {
                align_straight(r, &mut s, &mut t);
            }
            let found = best_orth(&s, &t, hints, true).or_else(|| best_orth(&s, &t, hints, false));
            if let Some((cost, path)) = found {
                if best.as_ref().is_none_or(|(c, _)| cost < *c) {
                    best = Some((cost, path));
                }
            }
        }
    }
    match best {
        Some((_, path)) => path,
        None => {
            let s = s_fixed.map_or_else(|| sb.center(), |f| f.pt);
            let t = t_fixed.map_or_else(|| tb.center(), |f| f.pt);
            vec![s, (t.0, s.1), t]
        }
    }
}

/// Facing sides whose ranges overlap leave and arrive at one common line, which gives
/// the straight connector draw.io shows instead of a jog.
fn align_straight(r: &Resolved, s: &mut End, t: &mut End) {
    let facing_h =
        s.dir == Dir::E && t.dir == Dir::W && t.pt.0 > s.pt.0 || s.dir == Dir::W && t.dir == Dir::E && t.pt.0 < s.pt.0;
    let facing_v =
        s.dir == Dir::S && t.dir == Dir::N && t.pt.1 > s.pt.1 || s.dir == Dir::N && t.dir == Dir::S && t.pt.1 < s.pt.1;
    if !(facing_h || facing_v) {
        return;
    }
    let (sr, tr) = (s.boxr, t.boxr);
    let (s_lo, s_hi, t_lo, t_hi, s_c, t_c) = if facing_h {
        (sr.y, sr.bottom(), tr.y, tr.bottom(), sr.cy(), tr.cy())
    } else {
        (sr.x, sr.right(), tr.x, tr.right(), sr.cx(), tr.cx())
    };
    let coord = if s.fixed && t.fixed {
        return;
    } else if s.fixed {
        let c = if facing_h { s.pt.1 } else { s.pt.0 };
        if c < t_lo || c > t_hi {
            return;
        }
        c
    } else if t.fixed {
        let c = if facing_h { t.pt.1 } else { t.pt.0 };
        if c < s_lo || c > s_hi {
            return;
        }
        c
    } else if t_c >= s_lo && t_c <= s_hi {
        t_c
    } else if s_c >= t_lo && s_c <= t_hi {
        s_c
    } else if s_lo.max(t_lo) <= s_hi.min(t_hi) {
        (s_lo.max(t_lo) + s_hi.min(t_hi)) / 2.0
    } else {
        return;
    };
    if !s.fixed && r.source.is_some() {
        s.pt = side_point(r.source, s.boxr, s.dir, Some(coord));
    }
    if !t.fixed && r.target.is_some() {
        t.pt = side_point(r.target, t.boxr, t.dir, Some(coord));
    }
}

/// Removes repeated points and merges collinear ones. `None` when the path doubles
/// back on itself, which no orthogonal connector does.
fn normalize(points: &[Pt]) -> Option<Vec<Pt>> {
    let mut out: Vec<Pt> = Vec::with_capacity(points.len());
    for &p in points {
        if out.last().is_none_or(|&q| dist(p, q) > 0.01) {
            out.push(p);
        }
    }
    let mut i = 1;
    while i + 1 < out.len() {
        let (a, b, c) = (out[i - 1], out[i], out[i + 1]);
        let cross = (b.0 - a.0) * (c.1 - b.1) - (b.1 - a.1) * (c.0 - b.0);
        if cross.abs() < 0.01 {
            let dot = (b.0 - a.0) * (c.0 - b.0) + (b.1 - a.1) * (c.1 - b.1);
            if dot < 0.0 {
                return None;
            }
            out.remove(i);
        } else {
            i += 1;
        }
    }
    Some(out)
}

fn seg_hits_rect(a: Pt, b: Pt, r: Rect) -> bool {
    if r.w <= 1.0 || r.h <= 1.0 {
        return false;
    }
    let e = 0.5;
    let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
    let (y0, y1) = (a.1.min(b.1), a.1.max(b.1));
    x1 > r.x + e && x0 < r.right() - e && y1 > r.y + e && y0 < r.bottom() - e
}

fn dist_to_segment(p: Pt, a: Pt, b: Pt) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 < 1e-9 { 0.0 } else { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0) };
    dist(p, (a.0 + t * dx, a.1 + t * dy))
}

fn dedup_sorted(mut v: Vec<f32>) -> Vec<f32> {
    // Keep the first occurrence so preferred coordinates stay in front.
    let mut out: Vec<f32> = Vec::new();
    for x in v.drain(..) {
        if !out.iter().any(|y| (x - y).abs() < 0.01) {
            out.push(x);
        }
    }
    out
}

/// Cheapest valid orthogonal path from `s` to `t` through the hints: fewest segments
/// first, then shortest, with channel positions preferring the middle between the
/// shapes.
fn best_orth(s: &End, t: &End, hints: &[Pt], avoid: bool) -> Option<(f32, Vec<Pt>)> {
    let (sr, tr) = (s.boxr, t.boxr);
    let gap_mid = |s_lo: f32, s_hi: f32, t_lo: f32, t_hi: f32, sp: f32, tp: f32| {
        if s_hi <= t_lo {
            (s_hi + t_lo) / 2.0
        } else if t_hi <= s_lo {
            (t_hi + s_lo) / 2.0
        } else {
            (sp + tp) / 2.0
        }
    };
    let mid_x = gap_mid(sr.x, sr.right(), tr.x, tr.right(), s.pt.0, t.pt.0);
    let mid_y = gap_mid(sr.y, sr.bottom(), tr.y, tr.bottom(), s.pt.1, t.pt.1);

    let mut xs = vec![mid_x];
    let mut ys = vec![mid_y];
    for h in hints {
        xs.push(h.0);
        ys.push(h.1);
    }
    let sv = s.dir.vec();
    let tv = t.dir.vec();
    xs.extend([
        s.pt.0 + sv.0 * JETTY,
        t.pt.0 + tv.0 * JETTY,
        sr.right() + JETTY,
        sr.x - JETTY,
        tr.right() + JETTY,
        tr.x - JETTY,
        s.pt.0,
        t.pt.0,
        s.pt.0 + JETTY,
        s.pt.0 - JETTY,
        t.pt.0 + JETTY,
        t.pt.0 - JETTY,
    ]);
    ys.extend([
        s.pt.1 + sv.1 * JETTY,
        t.pt.1 + tv.1 * JETTY,
        sr.bottom() + JETTY,
        sr.y - JETTY,
        tr.bottom() + JETTY,
        tr.y - JETTY,
        s.pt.1,
        t.pt.1,
        s.pt.1 + JETTY,
        s.pt.1 - JETTY,
        t.pt.1 + JETTY,
        t.pt.1 - JETTY,
    ]);
    let (xs, ys) = (dedup_sorted(xs), dedup_sorted(ys));

    let ax_s = s.dir.horizontal();
    let ax_t = t.dir.horizontal();
    let counts: &[usize] = if ax_s == ax_t { &[1, 3, 5] } else { &[2, 4] };
    let mut best: Option<(f32, Vec<Pt>)> = None;

    for &n in counts {
        // Segment i (1-based) runs horizontally when `horizontal(i)`; its fixed
        // coordinate is y for horizontal segments and x for vertical ones.
        let horizontal = |i: usize| if i % 2 == 1 { ax_s } else { !ax_s };
        let free = n.saturating_sub(2);
        let sets: Vec<&Vec<f32>> = (0..free).map(|k| if horizontal(k + 2) { &ys } else { &xs }).collect();
        let mut idx = vec![0usize; free];
        loop {
            // Coordinates of every segment's line.
            let mut c = Vec::with_capacity(n);
            c.push(if horizontal(1) { s.pt.1 } else { s.pt.0 });
            for (k, set) in sets.iter().enumerate() {
                c.push(set[idx[k]]);
            }
            if n > 1 {
                c.push(if horizontal(n) { t.pt.1 } else { t.pt.0 });
            }
            let ok_single = n != 1 || ((if horizontal(1) { t.pt.1 } else { t.pt.0 }) - c[0]).abs() < 0.5;
            if ok_single {
                let mut pts = vec![s.pt];
                for i in 1..n {
                    // Corner between segment i and i+1.
                    let (along, other) = (c[i], c[i - 1]);
                    pts.push(if horizontal(i) { (along, other) } else { (other, along) });
                }
                pts.push(t.pt);
                if let Some(cost) = score(&pts, s, t, hints, avoid) {
                    if best.as_ref().is_none_or(|(b, _)| cost.0 < *b) {
                        best = Some(cost);
                    }
                }
            }
            // Next combination.
            let mut k = 0;
            loop {
                if k == free {
                    break;
                }
                idx[k] += 1;
                if idx[k] < sets[k].len() {
                    break;
                }
                idx[k] = 0;
                k += 1;
            }
            if k == free {
                break;
            }
        }
        // Fewer segments always win when valid, so stop at the first count that works.
        if best.is_some() && hints.is_empty() {
            break;
        }
    }
    best
}

fn score(raw: &[Pt], s: &End, t: &End, hints: &[Pt], avoid: bool) -> Option<(f32, Vec<Pt>)> {
    let pts = normalize(raw)?;
    if pts.len() < 2 {
        return None;
    }
    let sv = s.dir.vec();
    let tv = t.dir.vec();
    let first = (pts[1].0 - pts[0].0, pts[1].1 - pts[0].1);
    let n = pts.len();
    let last = (pts[n - 2].0 - pts[n - 1].0, pts[n - 2].1 - pts[n - 1].1);
    // Leave along the exit direction and arrive from the entry side.
    if first.0 * sv.0 + first.1 * sv.1 < MIN_LEG - 1e-3 {
        return None;
    }
    if last.0 * tv.0 + last.1 * tv.1 < MIN_LEG - 1e-3 {
        return None;
    }
    if avoid {
        for i in 0..n - 1 {
            let (a, b) = (pts[i], pts[i + 1]);
            if i > 0 && seg_hits_rect(a, b, s.boxr) {
                return None;
            }
            if i < n - 2 && seg_hits_rect(a, b, t.boxr) {
                return None;
            }
        }
    }
    let length: f32 = pts.windows(2).map(|w| dist(w[0], w[1])).sum();
    let hint_miss: f32 =
        hints.iter().map(|&h| pts.windows(2).map(|w| dist_to_segment(h, w[0], w[1])).fold(f32::MAX, f32::min)).sum();
    let cost = hint_miss * 1000.0 + (n - 1) as f32 * 10_000.0 + length;
    Some((cost, pts))
}

/// Port of mxEdgeStyle.ElbowConnector (SideToSide and TopToBottom).
fn elbow(r: &Resolved, vertical: bool) -> Vec<Pt> {
    if r.is_loop() {
        return loop_route(r);
    }
    let (sb, tb) = (r.s_box(), r.t_box());
    let hint = r.waypoints.first().copied();
    let contains = |b: Rect, p: Pt| b.w > 0.0 && b.h > 0.0 && b.contains(p);
    let mut mid: Vec<Pt> = Vec::new();

    if !vertical {
        let l = sb.x.max(tb.x);
        let rgt = sb.right().min(tb.right());
        let x = hint.map_or((rgt + (l - rgt) / 2.0).round(), |h| h.0);
        let mut y1 = sb.cy();
        let mut y2 = tb.cy();
        if let Some(h) = hint {
            if h.1 >= sb.y && h.1 <= sb.bottom() {
                y1 = h.1;
            }
            if h.1 >= tb.y && h.1 <= tb.bottom() {
                y2 = h.1;
            }
        }
        if !contains(tb, (x, y1)) && !contains(sb, (x, y1)) {
            mid.push((x, y1));
        }
        if !contains(tb, (x, y2)) && !contains(sb, (x, y2)) {
            mid.push((x, y2));
        }
        if mid.len() == 1 {
            match hint {
                Some(h) => {
                    if !contains(tb, (x, h.1)) && !contains(sb, (x, h.1)) {
                        mid.push((x, h.1));
                    }
                }
                None => {
                    let top = sb.y.max(tb.y);
                    let bottom = sb.bottom().min(tb.bottom());
                    mid.push((x, top + (bottom - top) / 2.0));
                }
            }
        }
    } else {
        let top = sb.y.max(tb.y);
        let bottom = sb.bottom().min(tb.bottom());
        let y = hint.map_or((bottom + (top - bottom) / 2.0).round(), |h| h.1);
        let mut x1 = sb.cx();
        let mut x2 = tb.cx();
        if let Some(h) = hint {
            if h.0 >= sb.x && h.0 <= sb.right() {
                x1 = h.0;
            }
            if h.0 >= tb.x && h.0 <= tb.right() {
                x2 = h.0;
            }
        }
        if !contains(tb, (x1, y)) && !contains(sb, (x1, y)) {
            mid.push((x1, y));
        }
        if !contains(tb, (x2, y)) && !contains(sb, (x2, y)) {
            mid.push((x2, y));
        }
        if mid.len() == 1 {
            match hint {
                Some(h) => {
                    if !contains(tb, (h.0, y)) && !contains(sb, (h.0, y)) {
                        mid.push((h.0, y));
                    }
                }
                None => {
                    let left = sb.x.max(tb.x);
                    let right = sb.right().min(tb.right());
                    mid.push((left + (right - left) / 2.0, y));
                }
            }
        }
    }

    let s_fixed = r.s_fixed();
    let t_fixed = r.t_fixed();
    let first_next = mid.first().copied().unwrap_or_else(|| tb.center());
    let last_next = mid.last().copied().unwrap_or_else(|| sb.center());
    let s = match (&s_fixed, r.source) {
        (Some(f), _) => f.pt,
        (None, Some(t)) => t.outline.hit_orthogonal(t.rect.center(), first_next),
        (None, None) => r.sp.unwrap_or_else(|| sb.center()),
    };
    let t = match (&t_fixed, r.target) {
        (Some(f), _) => f.pt,
        (None, Some(t)) => t.outline.hit_orthogonal(t.rect.center(), last_next),
        (None, None) => r.tp.unwrap_or_else(|| tb.center()),
    };
    let mut pts = vec![s];
    pts.extend(mid);
    pts.push(t);
    normalize(&pts).unwrap_or(pts)
}

/// Port of mxEdgeStyle.EntityRelation.
fn entity_relation(r: &Resolved) -> Vec<Pt> {
    let (sb, tb) = (r.s_box(), r.t_box());
    let segment = r.style.num("segment", 30.0);
    let s_fixed = r.s_fixed();
    let t_fixed = r.t_fixed();
    let source_left = match &s_fixed {
        Some(f) => f.pt.0 <= sb.cx(),
        None => tb.right() < sb.x,
    };
    let target_left = match &t_fixed {
        Some(f) => f.pt.0 <= tb.cx(),
        None => sb.right() < tb.x,
    };
    let x0 = s_fixed.as_ref().map_or(if source_left { sb.x } else { sb.right() }, |f| f.pt.0);
    let y0 = s_fixed.as_ref().map_or(sb.cy(), |f| f.pt.1);
    let xe = t_fixed.as_ref().map_or(if target_left { tb.x } else { tb.right() }, |f| f.pt.0);
    let ye = t_fixed.as_ref().map_or(tb.cy(), |f| f.pt.1);

    let dep = (x0 + if source_left { -segment } else { segment }, y0);
    let arr = (xe + if target_left { -segment } else { segment }, ye);
    let mut pts = vec![(x0, y0)];
    if source_left == target_left {
        let x = if source_left { x0.min(xe) - segment } else { x0.max(xe) + segment };
        pts.push((x, y0));
        pts.push((x, ye));
    } else if (dep.0 < arr.0) == source_left {
        let mid_y = y0 + (ye - y0) / 2.0;
        pts.extend([dep, (dep.0, mid_y), (arr.0, mid_y), arr]);
    } else {
        pts.extend([dep, arr]);
    }
    pts.push((xe, ye));
    // The terminals sit where the connector leaves horizontally, on the perimeter.
    if let (None, Some(t)) = (&s_fixed, r.source) {
        pts[0] = t.outline.hit_ray(sb.center(), pts[1]);
    }
    if let (None, Some(t)) = (&t_fixed, r.target) {
        let n = pts.len();
        pts[n - 1] = t.outline.hit_ray(tb.center(), pts[n - 2]);
    }
    pts
}

// Label anchor --------------------------------------------------------------------------

/// The point `gx` (-1 start .. 1 end, along the route) and `gy` (pixels to the right of
/// the direction of travel) describe, as draw.io places edge labels.
pub fn point_at(pts: &[Pt], gx: f32, gy: f32) -> Pt {
    if pts.is_empty() {
        return (0.0, 0.0);
    }
    if pts.len() == 1 {
        return pts[0];
    }
    let lens: Vec<f32> = pts.windows(2).map(|w| dist(w[0], w[1])).collect();
    let total: f32 = lens.iter().sum();
    let target = ((gx.clamp(-1.0, 1.0) + 1.0) / 2.0) * total;
    let mut acc = 0.0;
    let mut seg = lens.len() - 1;
    for (i, l) in lens.iter().enumerate() {
        if acc + l >= target || i == lens.len() - 1 {
            seg = i;
            break;
        }
        acc += l;
    }
    let (a, b) = (pts[seg], pts[seg + 1]);
    let len = lens[seg].max(1e-6);
    let t = ((target - acc) / len).clamp(0.0, 1.0);
    let (ux, uy) = ((b.0 - a.0) / len, (b.1 - a.1) / len);
    (a.0 + (b.0 - a.0) * t - uy * gy, a.1 + (b.1 - a.1) * t + ux * gy)
}

// Drawing -------------------------------------------------------------------------------

pub fn draw(scene: &mut Scene, style: &Style, route: &[Pt]) {
    if route.len() < 2 {
        return;
    }
    let Some(stroke) = shapes::stroke_of(style, "strokeColor", "#000000") else { return };
    let opacity = (style.num("opacity", 100.0) / 100.0).clamp(0.0, 1.0);
    let sw = stroke.width;
    let mut pts: Vec<Pt> = route.to_vec();

    let marker_stroke = Stroke::new(stroke.paint.clone(), sw).join(Join::Miter);
    let mut parts: Vec<MarkerPart> = Vec::new();
    for source in [true, false] {
        let name =
            style.str_or(if source { "startArrow" } else { "endArrow" }, if source { "none" } else { "classic" });
        if name == "none" {
            continue;
        }
        let default_size = if name.starts_with("ER") { 10.0 } else { 6.0 };
        let size = style.num(if source { "startSize" } else { "endSize" }, default_size);
        let filled = style.flag(if source { "startFill" } else { "endFill" }, true);
        let n = pts.len();
        let end = if source { 0 } else { n - 1 };
        // Direction from the nearest distinct neighbor to the tip.
        let mut k = 1;
        let mut from = pts[if source { 1 } else { n - 2 }];
        while k < n - 1 && dist(from, pts[end]) < 0.5 {
            k += 1;
            from = pts[if source { k } else { n - 1 - k }];
        }
        let d = dist(pts[end], from).max(1.0);
        let u = ((pts[end].0 - from.0) / d, (pts[end].1 - from.1) / d);
        let mut pe = pts[end];
        parts.extend(marker(name, &mut pe, u, size, sw, source, filled));
        pts[end] = pe;
    }

    let rounded = style.flag("rounded", false);
    let curved = style.flag("curved", false);
    let arc = style.num("arcSize", 20.0);
    let line_style = SceneStyle::new().opacity(opacity).stroke(stroke.clone());
    scene.push_path(polyline_path(&pts, rounded, curved, arc), line_style);

    let fill_paint = stroke.paint.clone();
    for part in parts {
        let stroke_only = SceneStyle::new().opacity(opacity).stroke(marker_stroke.clone());
        match part {
            MarkerPart::Poly { pts, filled } => {
                let st = if filled {
                    SceneStyle::new().opacity(opacity).fill(fill_paint.clone()).stroke(marker_stroke.clone())
                } else {
                    stroke_only
                };
                scene.push_polygon(&pts, st);
            }
            MarkerPart::Lines(lines) => {
                let mut pb = PathBuilder::new();
                for (a, b) in lines {
                    pb.move_to(a.0, a.1).line_to(b.0, b.1);
                }
                scene.push_path(pb, stroke_only.stroke(marker_stroke.clone().cap(Cap::Butt)));
            }
            MarkerPart::Open(points) => scene.push_polyline(&points, marker_stroke.clone()),
            MarkerPart::Circle { c, r, filled } => {
                let st = if filled {
                    SceneStyle::new().opacity(opacity).fill(fill_paint.clone()).stroke(marker_stroke.clone())
                } else {
                    stroke_only
                };
                scene.push_ellipse(c.0 - r, c.1 - r, 2.0 * r, 2.0 * r, st);
            }
            MarkerPart::Arc { c, r, n, u } => {
                let mut pb = PathBuilder::new();
                for i in 0..=16 {
                    let t = PI * i as f32 / 16.0;
                    let p = (c.0 + n.0 * r * t.cos() - u.0 * r * t.sin(), c.1 + n.1 * r * t.cos() - u.1 * r * t.sin());
                    if i == 0 {
                        pb.move_to(p.0, p.1);
                    } else {
                        pb.line_to(p.0, p.1);
                    }
                }
                scene.push_path(pb, stroke_only.stroke(marker_stroke.clone()));
            }
        }
    }
}

/// draw.io's mxPolyline: straight, rounded corners (quadratic with the corner as the
/// control point) or a smooth curve through the points.
pub fn polyline_path(pts: &[Pt], rounded: bool, curved: bool, arc_size: f32) -> PathBuilder {
    let mut pb = PathBuilder::new();
    let n = pts.len();
    pb.move_to(pts[0].0, pts[0].1);
    if n == 2 {
        pb.line_to(pts[1].0, pts[1].1);
        return pb;
    }
    if curved {
        for i in 1..n.saturating_sub(2) {
            let (p0, p1) = (pts[i], pts[i + 1]);
            pb.quad_to(p0.0, p0.1, (p0.0 + p1.0) / 2.0, (p0.1 + p1.1) / 2.0);
        }
        let (p0, p1) = (pts[n - 2], pts[n - 1]);
        pb.quad_to(p0.0, p0.1, p1.0, p1.1);
        return pb;
    }
    let mut pt = pts[0];
    let mut i = 1;
    while i < n - 1 {
        let mut tmp = pts[i];
        let (dx, dy) = (pt.0 - tmp.0, pt.1 - tmp.1);
        if rounded && (dx != 0.0 || dy != 0.0) {
            let d = (dx * dx + dy * dy).sqrt();
            let k = arc_size.min(d / 2.0) / d;
            pb.line_to(tmp.0 + dx * k, tmp.1 + dy * k);
            let mut next = pts[i + 1];
            while i < n - 2 && (next.0 - tmp.0).round() == 0.0 && (next.1 - tmp.1).round() == 0.0 {
                next = pts[i + 2];
                i += 1;
            }
            let (dx, dy) = (next.0 - tmp.0, next.1 - tmp.1);
            let d = (dx * dx + dy * dy).sqrt().max(1.0);
            let k = arc_size.min(d / 2.0) / d;
            let (x2, y2) = (tmp.0 + dx * k, tmp.1 + dy * k);
            pb.quad_to(tmp.0, tmp.1, x2, y2);
            tmp = (x2, y2);
        } else {
            pb.line_to(tmp.0, tmp.1);
        }
        pt = tmp;
        i += 1;
    }
    pb.line_to(pts[n - 1].0, pts[n - 1].1);
    pb
}

enum MarkerPart {
    Poly {
        pts: Vec<Pt>,
        filled: bool,
    },
    Open(Vec<Pt>),
    Lines(Vec<(Pt, Pt)>),
    Circle {
        c: Pt,
        r: f32,
        filled: bool,
    },
    /// Half circle bulging against the direction of travel `u`.
    Arc {
        c: Pt,
        r: f32,
        n: Pt,
        u: Pt,
    },
}

/// Ports of draw.io's mxMarker shapes. `pe` is the line end, moved back so the line
/// stops at the arrow base; `u` points along the line toward the tip.
fn marker(name: &str, pe: &mut Pt, u: Pt, size: f32, sw: f32, source: bool, filled: bool) -> Vec<MarkerPart> {
    let n = (-u.1, u.0);
    let at = |p: Pt, along: f32, across: f32| (p.0 + u.0 * along + n.0 * across, p.1 + u.1 * along + n.1 * across);
    match name {
        "classic" | "classicThin" | "block" | "blockThin" => {
            let wf = if name.ends_with("Thin") { 3.0 } else { 2.0 };
            let classic = name.starts_with("classic");
            let off = sw * 1.118;
            let len = size + sw;
            let tip = at(*pe, -off, 0.0);
            let f = if classic { 0.75 } else { 1.0 };
            *pe = at(*pe, -len * f - off, 0.0);
            let mut pts = vec![tip, at(tip, -len, -len / wf)];
            if classic {
                pts.push(at(tip, -len * 0.75, 0.0));
            }
            pts.push(at(tip, -len, len / wf));
            vec![MarkerPart::Poly { pts, filled }]
        }
        "open" | "openThin" => {
            let wf = if name.ends_with("Thin") { 3.0 } else { 2.0 };
            let off = sw * 1.118;
            let len = size + sw;
            let tip = at(*pe, -off, 0.0);
            *pe = at(*pe, -off * 2.0, 0.0);
            vec![MarkerPart::Open(vec![at(tip, -len, -len / wf), tip, at(tip, -len, len / wf)])]
        }
        "oval" | "circle" => {
            let a = size / 2.0;
            let c = at(*pe, -0.5, 0.0);
            *pe = at(*pe, -a, 0.0);
            vec![MarkerPart::Circle { c, r: a, filled }]
        }
        "diamond" | "diamondThin" => {
            let thin = name == "diamondThin";
            let sf = if thin { 0.9862 } else { std::f32::consts::FRAC_1_SQRT_2 };
            let tk = if thin { 3.4 } else { 2.0 };
            let off = sw * sf;
            let len = size + sw;
            let tip = at(*pe, -off, 0.0);
            *pe = at(*pe, -len - off, 0.0);
            let pts = vec![tip, at(tip, -len / 2.0, -len / tk), at(tip, -len, 0.0), at(tip, -len / 2.0, len / tk)];
            vec![MarkerPart::Poly { pts, filled }]
        }
        "dash" => {
            let len = size + sw + 1.0;
            vec![MarkerPart::Lines(vec![(at(*pe, -len / 2.0, -len / 2.0), at(*pe, -len / 2.0, len / 2.0))])]
        }
        "cross" => {
            let len = size + sw + 1.0;
            vec![MarkerPart::Lines(vec![
                (at(*pe, -len * 0.0, -len / 2.0), at(*pe, -len, len / 2.0)),
                (at(*pe, -len, -len / 2.0), at(*pe, 0.0, len / 2.0)),
            ])]
        }
        "async" => {
            let off = sw * 1.118;
            let len = size + sw;
            let tip = at(*pe, -off, 0.0);
            *pe = at(*pe, -len - off, 0.0);
            let side = if source { len / 2.0 } else { -len / 2.0 };
            vec![MarkerPart::Poly { pts: vec![tip, at(tip, -len, side), at(tip, -len, 0.0)], filled }]
        }
        "openAsync" => {
            let len = size + sw;
            let side = if source { len / 2.0 } else { -len / 2.0 };
            vec![MarkerPart::Open(vec![*pe, at(*pe, -len, side)])]
        }
        "box" => {
            let len = size + sw;
            let tip = at(*pe, -sw * 0.5, 0.0);
            *pe = at(*pe, -len - sw * 0.5, 0.0);
            let pts = vec![
                at(tip, 0.0, -len / 2.0),
                at(tip, 0.0, len / 2.0),
                at(tip, -len, len / 2.0),
                at(tip, -len, -len / 2.0),
            ];
            vec![MarkerPart::Poly { pts, filled }]
        }
        "halfCircle" => {
            let r = (size + sw) / 2.0;
            let c = *pe;
            *pe = at(*pe, -r, 0.0);
            vec![MarkerPart::Arc { c, r, n, u }]
        }
        "circlePlus" => {
            let r = size / 2.0 + sw;
            let c = at(*pe, -r, 0.0);
            *pe = at(*pe, -2.0 * r, 0.0);
            vec![
                MarkerPart::Circle { c, r, filled },
                MarkerPart::Lines(vec![(at(c, -r, 0.0), at(c, r, 0.0)), (at(c, 0.0, -r), at(c, 0.0, r))]),
            ]
        }
        // Entity-relation notation. `l` is the reach of the symbol along the line.
        "ERone" => {
            let l = size;
            vec![MarkerPart::Lines(vec![(at(*pe, -l, -l / 2.0), at(*pe, -l, l / 2.0))])]
        }
        "ERmandOne" => {
            let l = size;
            vec![MarkerPart::Lines(vec![
                (at(*pe, -l * 0.5, -l / 2.0), at(*pe, -l * 0.5, l / 2.0)),
                (at(*pe, -l, -l / 2.0), at(*pe, -l, l / 2.0)),
            ])]
        }
        "ERmany" => {
            let l = size;
            let apex = at(*pe, -l, 0.0);
            *pe = apex;
            vec![MarkerPart::Lines(vec![
                (apex, at(apex, l, -l / 2.0)),
                (apex, at(apex, l, 0.0)),
                (apex, at(apex, l, l / 2.0)),
            ])]
        }
        "ERoneToMany" => {
            let l = size;
            let apex = at(*pe, -l, 0.0);
            let tip = *pe;
            *pe = apex;
            vec![MarkerPart::Lines(vec![
                (apex, at(tip, 0.0, -l / 2.0)),
                (apex, tip),
                (apex, at(tip, 0.0, l / 2.0)),
                (at(tip, -l * 1.4, -l / 2.0), at(tip, -l * 1.4, l / 2.0)),
            ])]
        }
        "ERzeroToOne" => {
            let l = size;
            let r = l / 2.0;
            let tip = *pe;
            let c = at(tip, -l * 1.5, 0.0);
            *pe = at(tip, -l * 1.5 - r, 0.0);
            vec![
                MarkerPart::Lines(vec![(at(tip, -l * 0.5, -l / 2.0), at(tip, -l * 0.5, l / 2.0))]),
                MarkerPart::Circle { c, r, filled },
            ]
        }
        "ERzeroToMany" => {
            let l = size;
            let r = l / 2.0;
            let tip = *pe;
            let apex = at(tip, -l, 0.0);
            let c = at(tip, -l - r, 0.0);
            *pe = at(tip, -l - 2.0 * r, 0.0);
            vec![
                MarkerPart::Lines(vec![(apex, at(tip, 0.0, -l / 2.0)), (apex, tip), (apex, at(tip, 0.0, l / 2.0))]),
                MarkerPart::Circle { c, r, filled },
            ]
        }
        // Unknown names behave like draw.io's default end arrow.
        _ => marker("classic", pe, u, size, sw, source, filled),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Geom, Item, PathCmd};

    fn term(style: &str, x: f32, y: f32, w: f32, h: f32) -> Terminal<'static> {
        // Leaked so the terminal can outlive this helper; fine for a test.
        Terminal::new(Box::leak(Box::new(Style::parse(style))), Rect::new(x, y, w, h))
    }

    fn spec<'a>(style: &'a Style, s: &'a Terminal, t: &'a Terminal, waypoints: &'a [Pt]) -> EdgeSpec<'a> {
        EdgeSpec { style, source: Some(s), target: Some(t), waypoints, source_point: None, target_point: None, self_loop: false }
    }

    fn no_diagonals(pts: &[Pt]) {
        for w in pts.windows(2) {
            assert!(
                (w[0].0 - w[1].0).abs() < 0.01 || (w[0].1 - w[1].1).abs() < 0.01,
                "diagonal segment {:?} -> {:?} in {pts:?}",
                w[0],
                w[1]
            );
        }
    }

    fn orth(style: &str, s: &Terminal, t: &Terminal, wp: &[Pt]) -> Vec<Pt> {
        let st = Style::parse(&format!("edgeStyle=orthogonalEdgeStyle;{style}"));
        route(&spec(&st, s, t, wp), &mut Vec::new())
    }

    #[test]
    fn straight_edge_ends_on_the_perimeters() {
        let a = term("", 0.0, 0.0, 100.0, 60.0);
        let b = term("ellipse", 300.0, 0.0, 100.0, 60.0);
        let st = Style::parse("");
        let pts = route(&spec(&st, &a, &b, &[]), &mut Vec::new());
        assert_eq!(pts.len(), 2);
        assert!((pts[0].0 - 100.0).abs() < 0.01 && (pts[0].1 - 30.0).abs() < 0.01);
        assert!((pts[1].0 - 300.0).abs() < 0.01);
    }

    #[test]
    fn dangling_edges_use_their_points() {
        let st = Style::parse("");
        let spec = EdgeSpec {
            style: &st,
            source: None,
            target: None,
            waypoints: &[],
            source_point: Some((10.0, 10.0)),
            target_point: Some((90.0, 50.0)),
            self_loop: false,
        };
        assert_eq!(route(&spec, &mut Vec::new()), vec![(10.0, 10.0), (90.0, 50.0)]);
    }

    #[test]
    fn exit_entry_constraints_pin_the_ends() {
        let a = term("", 0.0, 0.0, 100.0, 60.0);
        let b = term("", 300.0, 200.0, 100.0, 60.0);
        let st = Style::parse("edgeStyle=orthogonalEdgeStyle;exitX=0.5;exitY=1;entryX=0;entryY=0.5");
        let pts = route(&spec(&st, &a, &b, &[]), &mut Vec::new());
        assert_eq!(pts[0], (50.0, 60.0));
        assert_eq!(*pts.last().unwrap(), (300.0, 230.0));
        no_diagonals(&pts);
        // Leaves downward, enters from the left: an L shape.
        assert_eq!(pts.len(), 3, "{pts:?}");
        assert_eq!(pts[1], (50.0, 230.0));
    }

    #[test]
    fn orthogonal_z_route_between_facing_boxes() {
        let a = term("", 0.0, 0.0, 100.0, 60.0);
        let b = term("", 300.0, 150.0, 100.0, 60.0);
        let pts = orth("", &a, &b, &[]);
        no_diagonals(&pts);
        assert_eq!(pts.len(), 4, "{pts:?}");
        assert_eq!(pts[0], (100.0, 30.0));
        assert_eq!(pts[1].0, 200.0);
        assert_eq!(*pts.last().unwrap(), (300.0, 180.0));
    }

    #[test]
    fn orthogonal_aligned_boxes_get_a_straight_line() {
        let a = term("", 0.0, 0.0, 100.0, 60.0);
        let b = term("", 300.0, 20.0, 100.0, 60.0);
        let pts = orth("", &a, &b, &[]);
        assert_eq!(pts.len(), 2, "{pts:?}");
        assert_eq!(pts[0].1, pts[1].1);
    }

    #[test]
    fn orthogonal_vertical_tree_edge() {
        let a = term("", 300.0, 0.0, 120.0, 60.0);
        let b = term("", 100.0, 160.0, 120.0, 60.0);
        let pts = orth("", &a, &b, &[]);
        no_diagonals(&pts);
        assert_eq!(pts[0], (360.0, 60.0));
        assert_eq!(*pts.last().unwrap(), (160.0, 160.0));
        assert_eq!(pts.len(), 4);
        assert_eq!(pts[1].1, 110.0);
    }

    #[test]
    fn orthogonal_never_has_diagonals_for_any_layout() {
        let a = term("", 200.0, 200.0, 100.0, 60.0);
        let cases = [
            (200.0, 40.0),
            (400.0, 40.0),
            (400.0, 200.0),
            (400.0, 360.0),
            (200.0, 360.0),
            (0.0, 360.0),
            (0.0, 200.0),
            (0.0, 40.0),
            (220.0, 210.0),
            (310.0, 270.0),
        ];
        for (x, y) in cases {
            let b = term("ellipse", x, y, 100.0, 60.0);
            for style in [
                "",
                "exitX=1;exitY=0.5",
                "entryX=0.5;entryY=1",
                "exitX=0;exitY=0.5;entryX=0;entryY=0.5",
                "exitX=1;exitY=0.5;entryX=1;entryY=0.5",
            ] {
                for wp in [&[][..], &[(260.0, 120.0)][..], &[(150.0, 300.0), (320.0, 300.0)][..]] {
                    let pts = orth(style, &a, &b, wp);
                    assert!(pts.len() >= 2, "{x},{y} {style}");
                    no_diagonals(&pts);
                }
            }
        }
    }

    #[test]
    fn orthogonal_routes_around_when_sides_face_away() {
        let a = term("", 0.0, 0.0, 100.0, 60.0);
        let b = term("", 0.0, 200.0, 100.0, 60.0);
        // Both leave and enter on the right side: a U shape outside the boxes.
        let pts = orth("exitX=1;exitY=0.5;entryX=1;entryY=0.5", &a, &b, &[]);
        no_diagonals(&pts);
        assert_eq!(pts.len(), 4, "{pts:?}");
        assert!(pts[1].0 > 100.0 && pts[2].0 == pts[1].0);
    }

    #[test]
    fn waypoint_sets_the_channel() {
        let a = term("", 0.0, 0.0, 100.0, 60.0);
        let b = term("", 300.0, 150.0, 100.0, 60.0);
        let pts = orth("", &a, &b, &[(250.0, 90.0)]);
        no_diagonals(&pts);
        assert_eq!(pts[1].0, 250.0, "{pts:?}");
    }

    #[test]
    fn elbow_edge_styles() {
        let a = term("", 0.0, 0.0, 100.0, 60.0);
        let b = term("", 300.0, 150.0, 100.0, 60.0);
        let st = Style::parse("edgeStyle=elbowEdgeStyle;elbow=vertical");
        let pts = route(&spec(&st, &a, &b, &[]), &mut Vec::new());
        no_diagonals(&pts);
        assert_eq!(pts[0], (50.0, 60.0));
        assert_eq!(*pts.last().unwrap(), (350.0, 150.0));
        let st = Style::parse("edgeStyle=elbowEdgeStyle");
        let pts = route(&spec(&st, &a, &b, &[]), &mut Vec::new());
        no_diagonals(&pts);
        assert_eq!(pts[0], (100.0, 30.0));
        assert_eq!(*pts.last().unwrap(), (300.0, 180.0));
    }

    #[test]
    fn entity_relation_leaves_horizontally() {
        let a = term("", 0.0, 0.0, 100.0, 60.0);
        let b = term("", 300.0, 150.0, 100.0, 60.0);
        let st = Style::parse("edgeStyle=entityRelationEdgeStyle");
        let pts = route(&spec(&st, &a, &b, &[]), &mut Vec::new());
        // Like draw.io, the stubs are joined by a slanted segment.
        assert_eq!(pts, vec![(100.0, 30.0), (130.0, 30.0), (270.0, 180.0), (300.0, 180.0)]);
    }

    #[test]
    fn isometric_warns() {
        let a = term("", 0.0, 0.0, 100.0, 60.0);
        let b = term("", 300.0, 150.0, 100.0, 60.0);
        let st = Style::parse("edgeStyle=isometricEdgeStyle");
        let mut w = Vec::new();
        let pts = route(&spec(&st, &a, &b, &[]), &mut w);
        no_diagonals(&pts);
        assert!(w[0].contains("isometric"));
    }

    #[test]
    fn label_anchor_follows_the_route() {
        let pts = [(0.0, 0.0), (100.0, 0.0), (100.0, 100.0)];
        assert_eq!(point_at(&pts, 0.0, 0.0), (100.0, 0.0));
        assert_eq!(point_at(&pts, -1.0, 0.0), (0.0, 0.0));
        assert_eq!(point_at(&pts, 1.0, 0.0), (100.0, 100.0));
        let p = point_at(&[(0.0, 0.0), (100.0, 0.0)], 0.0, 10.0);
        assert_eq!(p, (50.0, 10.0));
    }

    fn drawn(style: &str, pts: &[Pt]) -> Scene {
        let mut s = Scene::new(1.0, 1.0);
        draw(&mut s, &Style::parse(style), pts);
        s
    }

    #[test]
    fn line_stops_at_the_arrow_base() {
        let s = drawn("endArrow=block;endFill=1", &[(0.0, 0.0), (100.0, 0.0)]);
        let Item::Shape(line) = &s.items[0] else { panic!() };
        let Geom::Path { cmds } = &line.geom else { panic!() };
        let PathCmd::Line { x, .. } = cmds[1] else { panic!() };
        // size 6 + stroke 1 plus the miter allowance
        assert!((x - (100.0 - 7.0 - 1.118)).abs() < 0.01, "{x}");
        assert_eq!(s.items.len(), 2);
    }

    #[test]
    fn classic_arrow_keeps_a_quarter_of_the_head_under_the_line() {
        let s = drawn("", &[(0.0, 0.0), (100.0, 0.0)]);
        let Item::Shape(line) = &s.items[0] else { panic!() };
        let Geom::Path { cmds } = &line.geom else { panic!() };
        let PathCmd::Line { x, .. } = cmds[1] else { panic!() };
        assert!((x - (100.0 - 7.0 * 0.75 - 1.118)).abs() < 0.01, "{x}");
    }

    #[test]
    fn every_marker_draws_something() {
        for name in [
            "classic",
            "classicThin",
            "block",
            "blockThin",
            "open",
            "openThin",
            "oval",
            "diamond",
            "diamondThin",
            "dash",
            "cross",
            "async",
            "openAsync",
            "box",
            "halfCircle",
            "circle",
            "circlePlus",
            "ERone",
            "ERmandOne",
            "ERmany",
            "ERoneToMany",
            "ERzeroToOne",
            "ERzeroToMany",
        ] {
            let s = drawn(&format!("startArrow={name};endArrow={name};startFill=0"), &[(0.0, 0.0), (100.0, 0.0)]);
            assert!(s.items.len() >= 3, "{name}: {}", s.items.len());
        }
        let s = drawn("endArrow=none", &[(0.0, 0.0), (100.0, 0.0)]);
        assert_eq!(s.items.len(), 1);
    }

    #[test]
    fn rounded_corners_use_quadratics() {
        let pb = polyline_path(&[(0.0, 0.0), (100.0, 0.0), (100.0, 100.0)], true, false, 20.0);
        let mut s = Scene::new(1.0, 1.0);
        s.push_path(pb, SceneStyle::new());
        let Item::Shape(sh) = &s.items[0] else { panic!() };
        let Geom::Path { cmds } = &sh.geom else { panic!() };
        assert!(cmds.iter().any(|c| matches!(c, PathCmd::Cubic { .. })));
        let PathCmd::Line { x, y } = cmds[1] else { panic!() };
        assert_eq!((x, y), (80.0, 0.0));
    }
}
