//! Drawing helpers shared by every diagram type: arrowheads, arrows, notes with a folded
//! corner, participant and actor icons, and moving finished items around.
//!
//! Colors are `#rrggbbaa` strings as produced by `Skin`. Lengths follow PlantUML's
//! pixel sizes, which are points here.

use crate::scene::{HAlign, Paint, PathBuilder, Scene, Stroke, Style};

pub fn solid(color: &str, width: f32) -> Stroke {
    Stroke::new(Paint::solid(color), width)
}

pub fn dashed(color: &str, width: f32, dash: &[f32]) -> Stroke {
    solid(color, width).dash(dash.to_vec())
}

/// Dash pattern of dashed message and flow arrows.
pub const ARROW_DASH: [f32; 2] = [4.0, 4.0];
/// Dash pattern of lifelines.
pub const LIFELINE_DASH: [f32; 2] = [5.0, 5.0];
/// Dotted pattern for the stretch of a lifeline inside a delay.
pub const DOTTED: [f32; 2] = [1.5, 3.0];

/// What sits at the end of an arrow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Head {
    None,
    /// Solid triangle with a notch, PlantUML's default `->`.
    Filled,
    /// Smaller solid head used on flow arrows (activity diagrams).
    Flow,
    /// Two strokes, `->>`.
    Open,
    /// Lower half of the filled head, `->\`.
    HalfLower,
    /// Upper half of the filled head, `->/`.
    HalfUpper,
    /// A cross, `->x` (lost message).
    Cross,
    /// A small ring, `->o`.
    Circle,
    /// Hollow triangle, inheritance in class diagrams.
    Triangle,
    /// Hollow diamond, aggregation.
    Diamond,
    /// Solid diamond, composition.
    DiamondFilled,
    /// Circle with a plus, nested classes.
    Plus,
    /// Hollow square.
    Square,
    /// Three prongs, many.
    Crowfoot,
    /// Ring touching the node, a provided interface.
    Lollipop,
    /// Arc around the end, a required interface.
    Socket,
}

const HEAD_LEN: f32 = 10.0;
const HEAD_HALF: f32 = 4.0;
const HEAD_NOTCH: f32 = 6.0;

fn unit(from: (f32, f32), to: (f32, f32)) -> (f32, f32) {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let l = (dx * dx + dy * dy).sqrt();
    if l < 1e-6 { (1.0, 0.0) } else { (dx / l, dy / l) }
}

/// How far the shaft has to stop short of the tip so it does not poke through the head.
pub fn head_inset(head: Head) -> f32 {
    match head {
        Head::Filled => HEAD_NOTCH,
        Head::Flow => 7.0,
        Head::Cross => 4.0,
        Head::Circle => 8.0,
        Head::Triangle => 18.0,
        Head::Diamond | Head::DiamondFilled => 24.0,
        Head::Plus => 16.0,
        Head::Square => 10.0,
        Head::Lollipop => 12.0,
        Head::Socket => 4.0,
        _ => 0.0,
    }
}

/// Draws one arrowhead with its tip at `tip`, traveling in direction `dir` (unit vector).
pub fn arrowhead(scene: &mut Scene, tip: (f32, f32), dir: (f32, f32), head: Head, color: &str, width: f32) {
    let (dx, dy) = dir;
    let (nx, ny) = (-dy, dx);
    let at = |back: f32, side: f32| (tip.0 - dx * back + nx * side, tip.1 - dy * back + ny * side);
    let fill = Style::new().fill(Paint::solid(color)).stroke(solid(color, 0.5));
    let hollow = Style::new().fill(Paint::solid("#FFFFFF")).stroke(solid(color, width));
    match head {
        Head::None => {}
        Head::Filled => scene.push_polygon(&[tip, at(HEAD_LEN, -HEAD_HALF), at(HEAD_NOTCH, 0.0), at(HEAD_LEN, HEAD_HALF)], fill),
        Head::HalfLower | Head::HalfUpper => {
            // "Lower" and "upper" are screen-relative whichever way the arrow points.
            let want = if head == Head::HalfLower { 1.0 } else { -1.0 };
            let s = want * if ny < 0.0 { -1.0 } else { 1.0 };
            scene.push_polygon(&[tip, at(HEAD_LEN, s * HEAD_HALF), at(HEAD_NOTCH, 0.0)], fill)
        }
        Head::Flow => scene.push_polygon(&[tip, at(10.0, -4.0), at(7.0, 0.0), at(10.0, 4.0)], fill),
        Head::Open => scene.push_polyline(&[at(HEAD_LEN, -HEAD_HALF), tip, at(HEAD_LEN, HEAD_HALF)], solid(color, width)),
        Head::Cross => {
            let c = at(4.0, 0.0);
            let r = 4.0;
            scene.push_polyline(&[(c.0 - r, c.1 - r), (c.0 + r, c.1 + r)], solid(color, width));
            scene.push_polyline(&[(c.0 - r, c.1 + r), (c.0 + r, c.1 - r)], solid(color, width));
        }
        Head::Circle => {
            let c = at(4.0, 0.0);
            scene.push_ellipse(c.0 - 4.0, c.1 - 4.0, 8.0, 8.0, Style::new().stroke(solid(color, width)));
        }
        Head::Triangle => scene.push_polygon(&[tip, at(18.0, -7.0), at(18.0, 7.0)], hollow),
        Head::Diamond => scene.push_polygon(&[tip, at(12.0, -6.0), at(24.0, 0.0), at(12.0, 6.0)], hollow),
        Head::DiamondFilled => scene.push_polygon(&[tip, at(12.0, -6.0), at(24.0, 0.0), at(12.0, 6.0)], fill),
        Head::Plus => {
            let c = at(8.0, 0.0);
            scene.push_ellipse(c.0 - 8.0, c.1 - 8.0, 16.0, 16.0, hollow);
            scene.push_polyline(&[at(8.0, -5.0), at(8.0, 5.0)], solid(color, width));
            scene.push_polyline(&[at(3.0, 0.0), at(13.0, 0.0)], solid(color, width));
        }
        Head::Square => scene.push_polygon(&[at(0.0, -5.0), at(10.0, -5.0), at(10.0, 5.0), at(0.0, 5.0)], hollow),
        Head::Crowfoot => {
            let root = at(10.0, 0.0);
            for side in [-6.0, 0.0, 6.0] {
                scene.push_polyline(&[root, at(0.0, side)], solid(color, width));
            }
        }
        Head::Lollipop => {
            let c = at(6.0, 0.0);
            scene.push_ellipse(c.0 - 6.0, c.1 - 6.0, 12.0, 12.0, hollow);
        }
        Head::Socket => {
            let c = at(8.0, 0.0);
            let arc: Vec<(f32, f32)> = (0..=8)
                .map(|i| {
                    let a = (i as f32 / 8.0 - 0.5) * std::f32::consts::PI;
                    (c.0 + (dx * a.cos() + nx * a.sin()) * 8.0, c.1 + (dy * a.cos() + ny * a.sin()) * 8.0)
                })
                .collect();
            scene.push_polyline(&arc, solid(color, width));
        }
    }
}

/// A polyline with heads at either end. The shaft is shortened where a head needs it.
pub fn arrow(scene: &mut Scene, pts: &[(f32, f32)], start: Head, end: Head, stroke: Stroke) {
    if pts.len() < 2 {
        return;
    }
    let color = stroke.paint.color.clone();
    let width = stroke.width;
    let mut p = pts.to_vec();
    let n = p.len();
    let end_dir = unit(p[n - 2], p[n - 1]);
    let start_dir = unit(p[1], p[0]);
    let (tip_end, tip_start) = (p[n - 1], p[0]);
    let cut = |pt: &mut (f32, f32), dir: (f32, f32), by: f32| {
        pt.0 -= dir.0 * by;
        pt.1 -= dir.1 * by;
    };
    cut(&mut p[n - 1], end_dir, head_inset(end));
    cut(&mut p[0], start_dir, head_inset(start));
    scene.push_polyline(&p, stroke);
    arrowhead(scene, tip_end, end_dir, end, &color, width);
    arrowhead(scene, tip_start, start_dir, start, &color, width);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteKind {
    /// Rectangle with a folded top right corner.
    Note,
    /// Hexagon, `hnote`.
    Hex,
    /// Plain rectangle, `rnote`.
    Rect,
}

pub const NOTE_FOLD: f32 = 10.0;

pub fn note_shape(scene: &mut Scene, kind: NoteKind, x: f32, y: f32, w: f32, h: f32, fill: Paint, border: &str, bw: f32) {
    let style = Style::new().fill(fill).stroke(solid(border, bw).join(crate::scene::Join::Miter));
    match kind {
        NoteKind::Note => {
            let f = NOTE_FOLD.min(w / 2.0).min(h / 2.0);
            scene.push_polygon(&[(x, y), (x + w - f, y), (x + w, y + f), (x + w, y + h), (x, y + h)], style);
            scene.push_polyline(&[(x + w - f, y), (x + w - f, y + f), (x + w, y + f)], solid(border, bw));
        }
        NoteKind::Hex => {
            let c = 8.0_f32.min(w / 3.0);
            scene.push_polygon(&[(x + c, y), (x + w - c, y), (x + w, y + h / 2.0), (x + w - c, y + h), (x + c, y + h), (x, y + h / 2.0)], style);
        }
        NoteKind::Rect => scene.push_rect(x, y, w, h, 0.0, style),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconKind {
    Actor,
    Boundary,
    Control,
    Entity,
    Database,
}

/// Width and height of an icon, excluding its name.
pub fn icon_size(kind: IconKind) -> (f32, f32) {
    match kind {
        IconKind::Actor => (28.0, 50.0),
        IconKind::Boundary => (30.0, 24.0),
        IconKind::Control => (24.0, 28.0),
        IconKind::Entity => (24.0, 28.0),
        IconKind::Database => (24.0, 32.0),
    }
}

/// An elliptical arc as two cubics, from the left end of the ellipse to the right end
/// along the bottom (`down`) or the top.
fn half_ellipse(p: &mut PathBuilder, cx: f32, cy: f32, rx: f32, ry: f32, down: bool) {
    let k = 0.5523;
    let s = if down { 1.0 } else { -1.0 };
    p.cubic_to(cx - rx, cy + s * ry * k, cx - rx * k, cy + s * ry, cx, cy + s * ry);
    p.cubic_to(cx + rx * k, cy + s * ry, cx + rx, cy + s * ry * k, cx + rx, cy);
}

/// Draws a participant icon centered on `cx` with its top at `top`.
pub fn draw_icon(scene: &mut Scene, kind: IconKind, cx: f32, top: f32, fill: &Paint, border: &str, lw: f32) {
    let st = || Style::new().fill(fill.clone()).stroke(solid(border, lw));
    let line = |scene: &mut Scene, pts: &[(f32, f32)]| scene.push_polyline(pts, solid(border, lw));
    match kind {
        IconKind::Actor => actor_figure(scene, cx, top, fill, border, lw),
        IconKind::Boundary => {
            let (cxx, cy, r) = (cx + 3.0, top + 12.0, 12.0);
            scene.push_ellipse(cxx - r, cy - r, 2.0 * r, 2.0 * r, st());
            line(scene, &[(cx - 15.0, cy), (cxx - r, cy)]);
            line(scene, &[(cx - 15.0, top + 2.0), (cx - 15.0, top + 22.0)]);
        }
        IconKind::Control => {
            let (cy, r) = (top + 16.0, 12.0);
            scene.push_ellipse(cx - r, cy - r, 2.0 * r, 2.0 * r, st());
            line(scene, &[(cx + 3.0, top), (cx - 2.0, top + 4.0), (cx + 3.0, top + 8.0)]);
        }
        IconKind::Entity => {
            let (cy, r) = (top + 12.0, 12.0);
            scene.push_ellipse(cx - r, cy - r, 2.0 * r, 2.0 * r, st());
            line(scene, &[(cx - r, top + 27.0), (cx + r, top + 27.0)]);
        }
        IconKind::Database => {
            let (rx, ry, h) = (12.0, 5.0, 32.0);
            // One closed outline: top arc, right side, bottom arc, left side.
            let mut outline = PathBuilder::new();
            outline.move_to(cx - rx, top + ry);
            half_ellipse(&mut outline, cx, top + ry, rx, ry, false);
            outline.line_to(cx + rx, top + h - ry);
            outline.cubic_to(cx + rx, top + h - ry + ry * 0.5523, cx + rx * 0.5523, top + h, cx, top + h);
            outline.cubic_to(cx - rx * 0.5523, top + h, cx - rx, top + h - ry + ry * 0.5523, cx - rx, top + h - ry);
            outline.close();
            scene.push_path(outline, st());
            let mut lid = PathBuilder::new();
            lid.move_to(cx - rx, top + ry);
            half_ellipse(&mut lid, cx, top + ry, rx, ry, true);
            scene.push_path(lid, Style::new().stroke(solid(border, lw)));
        }
    }
}

/// The stick figure of an actor, centered on `cx`, `top` is the top of the head.
pub fn actor_figure(scene: &mut Scene, cx: f32, top: f32, fill: &Paint, border: &str, lw: f32) {
    let head = 16.0;
    scene.push_ellipse(cx - head / 2.0, top + lw / 2.0, head, head, Style::new().fill(fill.clone()).stroke(solid(border, lw)));
    let neck = top + head + lw / 2.0;
    let hip = neck + 20.0;
    let s = || solid(border, lw);
    scene.push_polyline(&[(cx, neck), (cx, hip)], s());
    scene.push_polyline(&[(cx - 13.0, neck + 7.0), (cx + 13.0, neck + 7.0)], s());
    scene.push_polyline(&[(cx - 13.0, hip + 13.0), (cx, hip), (cx + 13.0, hip + 13.0)], s());
}

/// A filled circle with no outline.
pub fn dot(scene: &mut Scene, cx: f32, cy: f32, r: f32, color: &str) {
    scene.push_ellipse(cx - r, cy - r, 2.0 * r, 2.0 * r, Style::new().fill(Paint::solid(color)));
}

/// Left edge of a box of width `w` whose left edge, center or right edge is at `x`.
pub fn anchor_left(anchor: HAlign, x: f32, w: f32) -> f32 {
    match anchor {
        HAlign::Left => x,
        HAlign::Center => x - w / 2.0,
        HAlign::Right => x - w,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrow_stops_before_the_filled_head() {
        let mut s = Scene::new(10.0, 10.0);
        arrow(&mut s, &[(0.0, 0.0), (50.0, 0.0)], Head::None, Head::Filled, solid("#000000ff", 1.0));
        let crate::scene::Item::Shape(line) = &s.items[0] else { panic!() };
        assert_eq!(line.bbox[2], 44.0);
        assert_eq!(s.items.len(), 2);
    }
}
