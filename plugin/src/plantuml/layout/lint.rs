//! Checks a laid-out graph diagram for spots a reader would trip over: lines through
//! boxes or titles, labels that touch boxes or sit closer to another line than to their
//! own, and lines drawn over each other. Graphviz gives no guarantee against any of
//! these, so they are reported as warnings instead of passing unnoticed.
//!
//! The thresholds are deliberately loose: a warning should mean something looks wrong
//! when you look at the picture, not that two shapes are a fraction of a point too close.

use super::gdraw::GraphInfo;
use super::graph::{dist, rects_hit, Pt, Rect};

/// Beyond this many findings a diagram gets one summary line instead of a wall of text.
const MAX: usize = 8;

pub fn check(info: &GraphInfo) -> Vec<String> {
    let mut out = Vec::new();
    let polys: Vec<(usize, Vec<Pt>)> = info
        .routes
        .iter()
        .filter(|(li, _)| info.links.get(*li).is_some_and(|(a, b)| a != b))
        .map(|(li, r)| (*li, r.flatten()))
        .collect();
    let boxes: Vec<Bounds> = polys.iter().map(|(_, p)| bounds(p)).collect();

    for ((li, poly), b) in polys.iter().zip(&boxes) {
        let (from, to) = info.links[*li];
        for (n, r) in &info.nodes {
            if *n == from || *n == to {
                continue;
            }
            let core = grow(r, -3.0);
            if may_meet(b, &core, 0.0) && enters(poly, &core) {
                out.push(format!("{} runs through {}", link(info, *li), name(info, *n)));
            }
        }
        for (c, r) in &info.titles {
            if enters(poly, &grow(r, -1.0)) {
                out.push(format!("{} crosses the title of {}", link(info, *li), name(info, *c)));
            }
        }
    }

    for (i, (li, end, r)) in info.labels.iter().enumerate() {
        let what = || format!("the {} of {}", if *end { "multiplicity" } else { "label" }, link(info, *li));
        for (n, nr) in &info.nodes {
            if rects_hit(&grow(r, 1.0), nr) {
                out.push(format!("{} touches {}", what(), name(info, *n)));
            }
        }
        for (c, cr) in &info.clusters {
            if rects_hit(r, cr) && !inside(r, &grow(cr, -1.0)) {
                out.push(format!("{} sits on the border of {}", what(), name(info, *c)));
            }
        }
        for (lj, _, other) in &info.labels[i + 1..] {
            if rects_hit(&grow(r, -0.5), &grow(other, -0.5)) {
                out.push(format!("{} overlaps a label of {}", what(), link(info, *lj)));
            }
        }
        let probe = grow(r, -1.5);
        for (lj, poly) in &polys {
            if enters(poly, &probe) {
                out.push(format!("{} is crossed by {}", what(), if lj == li { "its own line".to_string() } else { link(info, *lj) }));
            }
        }
        if *end {
            continue;
        }
        let Some((_, own)) = polys.iter().find(|(lj, _)| lj == li) else { continue };
        let d_own = rect_to_poly(r, own);
        if d_own > 12.0 {
            out.push(format!("{} is {d_own:.0} pt away from its line", what()));
        }
        // A label can sit just as close to a neighbor without confusion when it is
        // still near its own line; only a clear preference for another line misleads.
        if let Some((lj, d)) = polys.iter().filter(|(lj, _)| lj != li).map(|(lj, p)| (*lj, rect_to_poly(r, p))).min_by(|a, b| a.1.total_cmp(&b.1)) {
            if d + 2.0 < d_own && d < 8.0 {
                out.push(format!("{} sits closer to {} than to its own line", what(), link(info, lj)));
            }
        }
    }

    for (i, (la, pa)) in polys.iter().enumerate() {
        for ((lb, pb), bb) in polys[i + 1..].iter().zip(&boxes[i + 1..]) {
            // Lines whose boxes are further apart than the tolerance share nothing.
            if !near_bounds(&boxes[i], bb, 2.0) {
                continue;
            }
            let shared = shared_length(pa, pb, 2.0);
            if shared > 15.0 {
                out.push(format!("{} and {} are drawn over each other for {shared:.0} pt", link(info, *la), link(info, *lb)));
            }
        }
    }

    out.dedup();
    if out.len() > MAX {
        let rest = out.len() - MAX + 1;
        out.truncate(MAX - 1);
        out.push(format!("{rest} more layout problems"));
    }
    out.into_iter().map(|m| format!("layout: {m}")).collect()
}

fn name(info: &GraphInfo, e: usize) -> String {
    match info.names.get(e).map(String::as_str) {
        Some("") | None => "an unnamed element".to_string(),
        Some(n) => format!("\"{n}\""),
    }
}

fn link(info: &GraphInfo, li: usize) -> String {
    let (a, b) = info.links[li];
    let label = info.link_labels.get(li).and_then(|l| l.as_deref()).map_or(String::new(), |l| format!(" (\"{l}\")"));
    let line = info.link_lines.get(li).map_or(String::new(), |n| format!(" on line {n}"));
    format!("link {} -> {}{label}{line}", name(info, a), name(info, b))
}

fn grow(r: &Rect, by: f32) -> Rect {
    Rect::new(r.x - by, r.y - by, (r.w + 2.0 * by).max(0.0), (r.h + 2.0 * by).max(0.0))
}

fn inside(inner: &Rect, outer: &Rect) -> bool {
    inner.x >= outer.x && inner.y >= outer.y && inner.right() <= outer.right() && inner.bottom() <= outer.bottom()
}

/// Points along the polyline about a point apart, so thin boxes are not skipped.
fn samples(poly: &[Pt]) -> impl Iterator<Item = Pt> + '_ {
    poly.windows(2).flat_map(|w| {
        let steps = dist(w[0], w[1]).ceil().max(1.0) as usize;
        (0..=steps).map(move |k| {
            let t = k as f32 / steps as f32;
            (w[0].0 + (w[1].0 - w[0].0) * t, w[0].1 + (w[1].1 - w[0].1) * t)
        })
    })
}

/// Whether the polyline passes through the inside of the rectangle.
pub(crate) fn enters(poly: &[Pt], r: &Rect) -> bool {
    if r.w <= 0.0 || r.h <= 0.0 {
        return false;
    }
    samples(poly).any(|(x, y)| x > r.x && x < r.right() && y > r.y && y < r.bottom())
}

pub(crate) fn dist_to_poly(p: Pt, poly: &[Pt]) -> f32 {
    poly.windows(2)
        .map(|w| {
            let (dx, dy) = (w[1].0 - w[0].0, w[1].1 - w[0].1);
            let len2 = dx * dx + dy * dy;
            let t = if len2 < 1e-6 { 0.0 } else { (((p.0 - w[0].0) * dx + (p.1 - w[0].1) * dy) / len2).clamp(0.0, 1.0) };
            dist(p, (w[0].0 + dx * t, w[0].1 + dy * t))
        })
        .fold(f32::MAX, f32::min)
}

/// Gap between a box and a polyline, zero when they touch.
pub(crate) fn rect_to_poly(r: &Rect, poly: &[Pt]) -> f32 {
    if enters(poly, r) {
        return 0.0;
    }
    let corners = [(r.x, r.y), (r.right(), r.y), (r.right(), r.bottom()), (r.x, r.bottom())];
    let to_sides = poly.iter().map(|&p| dist(p, (p.0.clamp(r.x, r.right()), p.1.clamp(r.y, r.bottom()))));
    let to_corners = corners.iter().map(|&c| dist_to_poly(c, poly));
    to_sides.chain(to_corners).fold(f32::MAX, f32::min)
}

/// Bounding box of a polyline: left, top, right, bottom. Inverted for an empty one, so
/// it meets nothing.
type Bounds = [f32; 4];

fn bounds(poly: &[Pt]) -> Bounds {
    poly.iter().fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |b, p| [b[0].min(p.0), b[1].min(p.1), b[2].max(p.0), b[3].max(p.1)])
}

/// Sampled points can land a rounding error outside the polyline's box; this slack keeps
/// the box tests below from skipping a pair the exact test would catch.
const SLACK: f32 = 0.5;

/// Whether a polyline with box `b` can come within `by` of the rectangle.
fn may_meet(b: &Bounds, r: &Rect, by: f32) -> bool {
    b[0] <= r.right() + by + SLACK && r.x - by - SLACK <= b[2] && b[1] <= r.bottom() + by + SLACK && r.y - by - SLACK <= b[3]
}

fn near_bounds(a: &Bounds, b: &Bounds, by: f32) -> bool {
    a[0] <= b[2] + by + SLACK && b[0] - by - SLACK <= a[2] && a[1] <= b[3] + by + SLACK && b[1] - by - SLACK <= a[3]
}

/// How long `a` runs within `tol` of `b`. Lines into the same box meet at its outline,
/// which costs a point or two here; that is why the caller's threshold is well above it.
fn shared_length(a: &[Pt], b: &[Pt], tol: f32) -> f32 {
    let (bx0, by0, bx1, by1) = b.iter().fold((f32::MAX, f32::MAX, f32::MIN, f32::MIN), |m, p| (m.0.min(p.0), m.1.min(p.1), m.2.max(p.0), m.3.max(p.1)));
    let near = |p: Pt| p.0 >= bx0 - tol && p.0 <= bx1 + tol && p.1 >= by0 - tol && p.1 <= by1 + tol && dist_to_poly(p, b) < tol;
    let pts: Vec<Pt> = samples(a).collect();
    pts.windows(2).filter(|w| near(w[0]) && near(w[1])).map(|w| dist(w[0], w[1])).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plantuml::layout::graph::Route;

    fn line(a: Pt, b: Pt) -> Route {
        Route { segs: vec![[a, a, b, b]], ..Route::default() }
    }

    fn info(names: &[&str], links: &[(usize, usize)]) -> GraphInfo {
        GraphInfo { names: names.iter().map(|s| s.to_string()).collect(), links: links.to_vec(), link_labels: vec![None; links.len()], link_lines: (1..=links.len()).collect(), ..GraphInfo::default() }
    }

    #[test]
    fn a_line_through_another_box_is_reported() {
        let mut g = info(&["A", "B", "C"], &[(0, 1)]);
        g.nodes = vec![(0, Rect::new(0.0, 0.0, 20.0, 20.0)), (1, Rect::new(0.0, 100.0, 20.0, 20.0)), (2, Rect::new(0.0, 50.0, 20.0, 20.0))];
        g.routes = vec![(0, line((10.0, 20.0), (10.0, 100.0)))];
        let w = check(&g);
        assert_eq!(w, vec!["layout: link \"A\" -> \"B\" on line 1 runs through \"C\"".to_string()]);
    }

    #[test]
    fn lines_on_top_of_each_other_are_reported_but_meeting_lines_are_not() {
        let mut g = info(&["A", "B", "C"], &[(0, 1), (2, 1), (0, 2)]);
        g.routes = vec![(0, line((0.0, 0.0), (0.0, 100.0))), (1, line((0.0, 40.0), (0.0, 100.0))), (2, line((0.0, 0.0), (50.0, 0.0)))];
        let w = check(&g);
        assert_eq!(w.len(), 1, "{w:?}");
        // 60 pt of shared line plus the tolerance at either end.
        assert!(w[0].contains("\"C\" -> \"B\" on line 2 are drawn over each other for 6"), "{w:?}");
    }

    #[test]
    fn a_label_closer_to_another_line_is_reported() {
        let mut g = info(&["A", "B", "C", "D"], &[(0, 1), (2, 3)]);
        g.link_labels[0] = Some("events".into());
        g.routes = vec![(0, line((0.0, 0.0), (0.0, 100.0))), (1, line((30.0, 0.0), (30.0, 100.0)))];
        g.labels = vec![(0, false, Rect::new(10.0, 45.0, 17.0, 10.0))];
        let w = check(&g);
        assert_eq!(w, vec!["layout: the label of link \"A\" -> \"B\" (\"events\") on line 1 sits closer to link \"C\" -> \"D\" on line 2 than to its own line".to_string()]);
    }

    #[test]
    fn long_reports_are_cut_short() {
        let n = 20;
        let mut g = info(&["A", "B", "C"], &vec![(0, 1); n]);
        g.nodes = vec![(2, Rect::new(-10.0, 40.0, 20.0, 20.0))];
        g.routes = (0..n).map(|i| (i, line((i as f32 * 0.1, 0.0), (i as f32 * 0.1, 100.0)))).collect();
        let w = check(&g);
        assert_eq!(w.len(), MAX);
        assert!(w.last().unwrap().ends_with("more layout problems"), "{w:?}");
    }
}
