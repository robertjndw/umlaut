//! Vertex shapes: fill and stroke from the style, the geometry of each draw.io shape,
//! and the outline edges attach to.

use std::f32::consts::PI;

use crate::scene::{Paint, PathBuilder, Scene, Stroke, Style as SceneStyle};

use super::geometry::{bounds_of, rotate_about, Pt, Rect};
use super::label::scale_alpha;
use super::style::Style;

/// Typst's `rotate` turns clockwise for positive angles, like draw.io's `rotation`.
pub fn scene_rotation(cw_degrees: f32) -> f32 {
    cw_degrees
}

// Fill and stroke ---------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Look {
    pub fill: Option<Paint>,
    pub stroke: Option<Stroke>,
    pub opacity: f32,
}

impl Look {
    pub fn of(style: &Style) -> Look {
        let opacity = (style.num("opacity", 100.0) / 100.0).clamp(0.0, 1.0);
        Look { fill: fill_paint(style), stroke: stroke_of(style, "strokeColor", "#000000"), opacity }
    }

    fn scene_style(&self, rotation: f32) -> SceneStyle {
        let mut s = SceneStyle::new().opacity(self.opacity).rotation(scene_rotation(rotation));
        if let Some(f) = &self.fill {
            s = s.fill(f.clone());
        }
        if let Some(st) = &self.stroke {
            s = s.stroke(st.clone());
        }
        s
    }

    fn stroke_only(&self) -> SceneStyle {
        let mut s = SceneStyle::new().opacity(self.opacity);
        if let Some(st) = &self.stroke {
            s = s.stroke(st.clone());
        }
        s
    }

    fn fill_only(&self) -> SceneStyle {
        let mut s = SceneStyle::new().opacity(self.opacity);
        if let Some(f) = &self.fill {
            s = s.fill(f.clone());
        }
        s
    }
}

fn fill_paint(style: &Style) -> Option<Paint> {
    let color = style.color("fillColor", Some("#ffffff"))?;
    paint_with_gradient(style, color, "gradientColor", "fillOpacity")
}

fn paint_with_gradient(style: &Style, color: String, gradient_key: &str, opacity_key: &str) -> Option<Paint> {
    let alpha = (style.num(opacity_key, 100.0) / 100.0).clamp(0.0, 1.0);
    let mut paint = Paint::solid(&scale_alpha(&color, alpha));
    if let Some(to) = style.color(gradient_key, None) {
        // draw.io's direction names where the gradient color ends up.
        let angle = match style.str_or("gradientDirection", "south") {
            "north" => 270.0,
            "east" => 0.0,
            "west" => 180.0,
            _ => 90.0,
        };
        paint = paint.gradient_to(&scale_alpha(&to, alpha), angle);
    }
    Some(paint)
}

pub fn stroke_of(style: &Style, color_key: &str, default_color: &str) -> Option<Stroke> {
    let width = style.num("strokeWidth", 1.0);
    let color = style.color(color_key, Some(default_color))?;
    if width <= 0.0 {
        return None;
    }
    let alpha = (style.num("strokeOpacity", 100.0) / 100.0).clamp(0.0, 1.0);
    let mut stroke = Stroke::new(Paint::solid(&scale_alpha(&color, alpha)), width);
    if style.flag("dashed", false) {
        stroke = stroke.dash(dash_pattern(style, width));
    }
    Some(stroke)
}

/// draw.io multiplies the pattern by the stroke width unless `fixDash` is set.
pub fn dash_pattern(style: &Style, width: f32) -> Vec<f32> {
    let mut pattern: Vec<f32> = style
        .str_or("dashPattern", "3 3")
        .split([' ', ','])
        .filter_map(|p| p.trim().parse::<f32>().ok())
        .filter(|v| v.is_finite() && *v >= 0.0)
        .collect();
    if pattern.is_empty() || pattern.iter().all(|v| *v == 0.0) {
        pattern = vec![3.0, 3.0];
    }
    if pattern.len() % 2 == 1 {
        let copy = pattern.clone();
        pattern.extend(copy);
    }
    let k = if style.flag("fixDash", false) { 1.0 } else { width.max(0.01) };
    let scaled: Vec<f32> = pattern.iter().map(|v| v * k).collect();
    // A huge pattern times a huge width overflows to infinity.
    if scaled.iter().all(|v| v.is_finite()) {
        scaled
    } else {
        vec![3.0, 3.0]
    }
}

// Shape kinds ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Rect,
    Ellipse,
    DoubleEllipse,
    Rhombus,
    Triangle,
    Hexagon,
    Parallelogram,
    Trapezoid,
    Step,
    Cylinder,
    Datastore,
    Cloud,
    Document,
    Process,
    Note,
    Card,
    Actor,
    UmlActor,
    Swimlane,
    Image,
    Line,
    Callout,
    InternalStorage,
    Folder,
    Cube,
    Partial,
    Lifeline,
    Unknown(String),
}

pub fn kind_of(style: &Style) -> Kind {
    let Some(name) = style.shape() else { return Kind::Rect };
    match name.to_ascii_lowercase().as_str() {
        "rectangle" | "rect" | "label" | "text" | "roundrect" => Kind::Rect,
        "ellipse" | "oval" | "circle" => Kind::Ellipse,
        "doubleellipse" => Kind::DoubleEllipse,
        "rhombus" | "diamond" => Kind::Rhombus,
        "triangle" => Kind::Triangle,
        "hexagon" => Kind::Hexagon,
        "parallelogram" => Kind::Parallelogram,
        "trapezoid" => Kind::Trapezoid,
        "step" => Kind::Step,
        "cylinder" | "cylinder2" | "cylinder3" => Kind::Cylinder,
        "datastore" => Kind::Datastore,
        "cloud" => Kind::Cloud,
        "document" => Kind::Document,
        "process" => Kind::Process,
        "note" | "note2" => Kind::Note,
        "card" => Kind::Card,
        "actor" => Kind::Actor,
        "umlactor" => Kind::UmlActor,
        "swimlane" | "table" => Kind::Swimlane,
        "image" => Kind::Image,
        "line" => Kind::Line,
        "callout" => Kind::Callout,
        "internalstorage" => Kind::InternalStorage,
        "folder" => Kind::Folder,
        "cube" => Kind::Cube,
        "partialrectangle" | "tablerow" | "tablecell" => Kind::Partial,
        "umllifeline" => Kind::Lifeline,
        _ => Kind::Unknown(display_name(name)),
    }
}

/// Stencil shapes carry their whole definition in the name; keep warnings readable.
fn display_name(name: &str) -> String {
    if name.starts_with("stencil(") {
        return "stencil(...)".to_string();
    }
    name.chars().take(60).collect()
}

// Transform from the shape's own frame to the page ------------------------------------

/// Shapes are described in a local frame of size `lw` x `lh` that already accounts for
/// `direction` (a north-facing triangle is an east-facing one turned by 270 degrees in a
/// box with swapped sides), then flipped and rotated around the cell center.
#[derive(Debug, Clone)]
pub struct Xf {
    pub rect: Rect,
    quarter: f32,
    flip_h: bool,
    flip_v: bool,
    pub rot: f32,
}

impl Xf {
    pub fn new(rect: Rect, style: &Style) -> Xf {
        let quarter = match style.str_or("direction", "east") {
            "south" => 90.0,
            "west" => 180.0,
            "north" => 270.0,
            _ => 0.0,
        };
        Xf {
            rect,
            quarter,
            flip_h: style.flag("flipH", false),
            flip_v: style.flag("flipV", false),
            rot: style.num("rotation", 0.0),
        }
    }

    fn swapped(&self) -> bool {
        self.quarter == 90.0 || self.quarter == 270.0
    }

    pub fn lw(&self) -> f32 {
        if self.swapped() {
            self.rect.h
        } else {
            self.rect.w
        }
    }

    pub fn lh(&self) -> f32 {
        if self.swapped() {
            self.rect.w
        } else {
            self.rect.h
        }
    }

    pub fn p(&self, lx: f32, ly: f32) -> Pt {
        let mut x = lx - self.lw() / 2.0;
        let mut y = ly - self.lh() / 2.0;
        if self.flip_h {
            x = -x;
        }
        if self.flip_v {
            y = -y;
        }
        let c = self.rect.center();
        rotate_about((c.0 + x, c.1 + y), c, self.quarter + self.rot)
    }
}

/// Path in the local frame, emitted through the transform.
struct Pen<'a> {
    xf: &'a Xf,
    pb: PathBuilder,
}

impl<'a> Pen<'a> {
    fn new(xf: &'a Xf) -> Pen<'a> {
        Pen { xf, pb: PathBuilder::new() }
    }

    fn m(&mut self, x: f32, y: f32) -> &mut Self {
        let (x, y) = self.xf.p(x, y);
        self.pb.move_to(x, y);
        self
    }

    fn l(&mut self, x: f32, y: f32) -> &mut Self {
        let (x, y) = self.xf.p(x, y);
        self.pb.line_to(x, y);
        self
    }

    fn c(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) -> &mut Self {
        let (a, b) = self.xf.p(x1, y1);
        let (c, d) = self.xf.p(x2, y2);
        let (e, f) = self.xf.p(x, y);
        self.pb.cubic_to(a, b, c, d, e, f);
        self
    }

    fn q(&mut self, x1: f32, y1: f32, x: f32, y: f32) -> &mut Self {
        let (a, b) = self.xf.p(x1, y1);
        let (c, d) = self.xf.p(x, y);
        self.pb.quad_to(a, b, c, d);
        self
    }

    fn z(&mut self) -> &mut Self {
        self.pb.close();
        self
    }

    /// Elliptical arc around (cx, cy) from angle `a0` to `a1` (radians, y down), as
    /// cubic segments of at most a quarter turn. Continues the current subpath.
    fn arc(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, a0: f32, a1: f32) -> &mut Self {
        let segments = ((a1 - a0).abs() / (PI / 2.0)).ceil().max(1.0) as usize;
        let step = (a1 - a0) / segments as f32;
        let k = 4.0 / 3.0 * (step / 4.0).tan();
        for i in 0..segments {
            let s = a0 + step * i as f32;
            let e = s + step;
            let (ss, sc) = s.sin_cos();
            let (es, ec) = e.sin_cos();
            self.c(
                cx + rx * (sc - k * ss),
                cy + ry * (ss + k * sc),
                cx + rx * (ec + k * es),
                cy + ry * (es - k * ec),
                cx + rx * ec,
                cy + ry * es,
            );
        }
        self
    }

    fn done(self) -> PathBuilder {
        self.pb
    }
}

// Outline for perimeter lookups ---------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Outline {
    Rect(Rect),
    Ellipse(Rect),
    Poly(Vec<Pt>),
}

impl Outline {
    pub fn bounds(&self) -> Rect {
        match self {
            Outline::Rect(r) | Outline::Ellipse(r) => *r,
            Outline::Poly(p) => bounds_of(p.iter().copied()),
        }
    }

    /// Where the ray from `from` (inside the shape) toward `toward` leaves it.
    pub fn hit_ray(&self, from: Pt, toward: Pt) -> Pt {
        let (dx, dy) = (toward.0 - from.0, toward.1 - from.1);
        if dx.abs() < 1e-6 && dy.abs() < 1e-6 {
            return from;
        }
        match self {
            Outline::Rect(r) => {
                let tx =
                    if dx.abs() < 1e-6 { f32::MAX } else { ((if dx > 0.0 { r.right() } else { r.x }) - from.0) / dx };
                let ty =
                    if dy.abs() < 1e-6 { f32::MAX } else { ((if dy > 0.0 { r.bottom() } else { r.y }) - from.1) / dy };
                let t = tx.min(ty);
                (from.0 + dx * t, from.1 + dy * t)
            }
            Outline::Ellipse(r) => {
                let (a, b) = (r.w / 2.0, r.h / 2.0);
                // A degenerate ellipse is a point (or a line); dividing by its radius
                // below would give NaN.
                if a < 1e-3 || b < 1e-3 {
                    return (r.cx(), r.cy());
                }
                let (ex, ey) = (from.0 - r.cx(), from.1 - r.cy());
                // Solve |((ex + t dx)/a, (ey + t dy)/b)| = 1 for the positive root.
                let qa = (dx / a).powi(2) + (dy / b).powi(2);
                let qb = 2.0 * (ex * dx / (a * a) + ey * dy / (b * b));
                let qc = (ex / a).powi(2) + (ey / b).powi(2) - 1.0;
                let disc = (qb * qb - 4.0 * qa * qc).max(0.0).sqrt();
                let t = (-qb + disc) / (2.0 * qa);
                (from.0 + dx * t, from.1 + dy * t)
            }
            Outline::Poly(pts) => {
                let mut best: Option<f32> = None;
                for i in 0..pts.len() {
                    let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
                    let (sx, sy) = (q.0 - p.0, q.1 - p.1);
                    let den = dx * sy - dy * sx;
                    if den.abs() < 1e-9 {
                        continue;
                    }
                    let t = ((p.0 - from.0) * sy - (p.1 - from.1) * sx) / den;
                    let u = ((p.0 - from.0) * dy - (p.1 - from.1) * dx) / den;
                    if t > 1e-6 && (-1e-6..=1.0 + 1e-6).contains(&u) {
                        best = Some(best.map_or(t, |b| b.max(t)));
                    }
                }
                match best {
                    Some(t) => (from.0 + dx * t, from.1 + dy * t),
                    None => from,
                }
            }
        }
    }

    /// Horizontal extent (min x, max x) of the outline on the line at height `y`.
    pub fn hline(&self, y: f32) -> Option<(f32, f32)> {
        match self {
            Outline::Rect(r) => (y >= r.y - 1e-3 && y <= r.bottom() + 1e-3).then_some((r.x, r.right())),
            Outline::Ellipse(r) => {
                let t = (y - r.cy()) / (r.h / 2.0);
                (t.abs() <= 1.0 + 1e-4).then(|| {
                    let d = r.w / 2.0 * (1.0 - t * t).max(0.0).sqrt();
                    (r.cx() - d, r.cx() + d)
                })
            }
            Outline::Poly(p) => poly_extent(p, y, true),
        }
    }

    /// Vertical extent (min y, max y) on the line at `x`.
    pub fn vline(&self, x: f32) -> Option<(f32, f32)> {
        match self {
            Outline::Rect(r) => (x >= r.x - 1e-3 && x <= r.right() + 1e-3).then_some((r.y, r.bottom())),
            Outline::Ellipse(r) => {
                let t = (x - r.cx()) / (r.w / 2.0);
                (t.abs() <= 1.0 + 1e-4).then(|| {
                    let d = r.h / 2.0 * (1.0 - t * t).max(0.0).sqrt();
                    (r.cy() - d, r.cy() + d)
                })
            }
            Outline::Poly(p) => poly_extent(p, x, false),
        }
    }

    /// draw.io's "orthogonal" perimeter point: when `next` lies within the shape's
    /// horizontal or vertical range the edge leaves straight toward it instead of
    /// aiming at the center.
    pub fn hit_orthogonal(&self, center: Pt, next: Pt) -> Pt {
        let b = self.bounds();
        if next.0 >= b.x && next.0 <= b.right() {
            if let Some((y0, y1)) = self.vline(next.0) {
                return (next.0, if next.1 < center.1 { y0 } else { y1 });
            }
        }
        if next.1 >= b.y && next.1 <= b.bottom() {
            if let Some((x0, x1)) = self.hline(next.1) {
                return (if next.0 < center.0 { x0 } else { x1 }, next.1);
            }
        }
        self.hit_ray(center, next)
    }
}

fn poly_extent(pts: &[Pt], c: f32, horizontal: bool) -> Option<(f32, f32)> {
    let mut lo = f32::MAX;
    let mut hi = f32::MIN;
    for i in 0..pts.len() {
        let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
        let (pa, pb, qa, qb) = if horizontal { (p.1, p.0, q.1, q.0) } else { (p.0, p.1, q.0, q.1) };
        if (pa - c) * (qa - c) > 0.0 || (pa - qa).abs() < 1e-9 && (pa - c).abs() > 1e-6 {
            continue;
        }
        let v = if (pa - qa).abs() < 1e-9 {
            // Edge lies along the line: both ends count.
            lo = lo.min(pb.min(qb));
            hi = hi.max(pb.max(qb));
            continue;
        } else {
            pb + (qb - pb) * (c - pa) / (qa - pa)
        };
        lo = lo.min(v);
        hi = hi.max(v);
    }
    (lo <= hi).then_some((lo, hi))
}

fn polygon_local(kind: &Kind, style: &Style, w: f32, h: f32) -> Option<Vec<Pt>> {
    // `fixedSize` switches `size` from a fraction of the width to pixels.
    let size = |default: f32| {
        if style.flag("fixedSize", false) {
            style.num("size", 20.0).clamp(0.0, w)
        } else {
            w * style.num("size", default).clamp(0.0, 1.0)
        }
    };
    Some(match kind {
        Kind::Rhombus => vec![(w / 2.0, 0.0), (w, h / 2.0), (w / 2.0, h), (0.0, h / 2.0)],
        Kind::Triangle => vec![(0.0, 0.0), (w, h / 2.0), (0.0, h)],
        Kind::Hexagon => {
            let s = size(0.25);
            vec![(s, 0.0), (w - s, 0.0), (w, h / 2.0), (w - s, h), (s, h), (0.0, h / 2.0)]
        }
        Kind::Parallelogram => {
            let s = size(0.2);
            vec![(0.0, h), (s, 0.0), (w, 0.0), (w - s, h)]
        }
        Kind::Trapezoid => {
            let s = size(0.2);
            vec![(0.0, h), (s, 0.0), (w - s, 0.0), (w, h)]
        }
        Kind::Step => {
            let s = size(0.25);
            vec![(0.0, 0.0), (w - s, 0.0), (w, h / 2.0), (w - s, h), (0.0, h), (s, h / 2.0)]
        }
        _ => return None,
    })
}

/// The outline edges attach to; follows the shape's `perimeter` or, without one, the
/// perimeter draw.io gives the shape.
pub fn outline(style: &Style, rect: Rect) -> Outline {
    let kind = kind_of(style);
    let xf = Xf::new(rect, style);
    let perimeter_kind = match style.get("perimeter") {
        Some("ellipsePerimeter") => Kind::Ellipse,
        Some("rhombusPerimeter") => Kind::Rhombus,
        Some("trianglePerimeter") => Kind::Triangle,
        Some("hexagonPerimeter2") | Some("hexagonPerimeter") => Kind::Hexagon,
        Some("parallelogramPerimeter") => Kind::Parallelogram,
        Some("trapezoidPerimeter") => Kind::Trapezoid,
        Some("stepPerimeter") => Kind::Step,
        Some("rectanglePerimeter") | Some("none") => Kind::Rect,
        _ => match kind {
            Kind::Cloud | Kind::DoubleEllipse => Kind::Ellipse,
            k => k,
        },
    };
    if let Some(local) = polygon_local(&perimeter_kind, style, xf.lw(), xf.lh()) {
        return Outline::Poly(local.into_iter().map(|(x, y)| xf.p(x, y)).collect());
    }
    let turned = xf.rot.rem_euclid(180.0);
    match perimeter_kind {
        Kind::Ellipse => {
            if turned == 0.0 {
                Outline::Ellipse(rect)
            } else if turned == 90.0 {
                let c = rect.center();
                Outline::Ellipse(Rect::new(c.0 - rect.h / 2.0, c.1 - rect.w / 2.0, rect.h, rect.w))
            } else {
                let (a, b, c) = (rect.w / 2.0, rect.h / 2.0, rect.center());
                Outline::Poly(
                    (0..48)
                        .map(|i| {
                            let t = i as f32 / 48.0 * 2.0 * PI;
                            rotate_about((c.0 + a * t.cos(), c.1 + b * t.sin()), c, xf.rot)
                        })
                        .collect(),
                )
            }
        }
        _ => {
            if xf.rot == 0.0 {
                Outline::Rect(rect)
            } else {
                let c = rect.center();
                let corners =
                    [(rect.x, rect.y), (rect.right(), rect.y), (rect.right(), rect.bottom()), (rect.x, rect.bottom())];
                Outline::Poly(corners.iter().map(|&p| rotate_about(p, c, xf.rot)).collect())
            }
        }
    }
}

// Drawing -------------------------------------------------------------------------------

pub struct Drawn {
    /// Where the label goes before spacing is applied.
    pub label: Rect,
}

pub fn draw(scene: &mut Scene, style: &Style, rect: Rect, look: &Look) -> Drawn {
    let kind = kind_of(style);
    let xf = Xf::new(rect, style);
    let (w, h) = (xf.lw(), xf.lh());
    let mut label = rect;

    match &kind {
        Kind::Rect => draw_rect(scene, style, rect, look),
        Kind::Ellipse => scene.push_ellipse(rect.x, rect.y, rect.w, rect.h, look.scene_style(xf.rot)),
        Kind::DoubleEllipse => {
            scene.push_ellipse(rect.x, rect.y, rect.w, rect.h, look.scene_style(xf.rot));
            let m = style.num("margin", (3.0 + stroke_width(look)).min(rect.w / 5.0).min(rect.h / 5.0));
            let inner = Look { fill: None, ..look.clone() };
            scene.push_ellipse(rect.x + m, rect.y + m, rect.w - 2.0 * m, rect.h - 2.0 * m, inner.scene_style(xf.rot));
        }
        Kind::Rhombus | Kind::Triangle | Kind::Hexagon | Kind::Parallelogram | Kind::Trapezoid | Kind::Step => {
            let pts: Vec<Pt> =
                polygon_local(&kind, style, w, h).unwrap().into_iter().map(|(x, y)| xf.p(x, y)).collect();
            scene.push_polygon(&pts, look.scene_style(0.0));
        }
        Kind::Cylinder => {
            let ry = draw_cylinder(scene, style, &xf, look);
            // `boundedLbl` keeps the label below the top ellipse.
            if style.flag("boundedLbl", false) {
                let top = 2.0 * ry;
                label = Rect::new(rect.x, rect.y + top, rect.w, (rect.h - top - ry).max(0.0));
            }
        }
        Kind::Datastore => draw_datastore(scene, &xf, look),
        Kind::Cloud => {
            let mut p = Pen::new(&xf);
            p.m(0.25 * w, 0.25 * h)
                .c(0.05 * w, 0.25 * h, 0.0, 0.5 * h, 0.16 * w, 0.55 * h)
                .c(0.0, 0.66 * h, 0.18 * w, 0.9 * h, 0.31 * w, 0.8 * h)
                .c(0.4 * w, h, 0.7 * w, h, 0.8 * w, 0.8 * h)
                .c(w, 0.8 * h, w, 0.6 * h, 0.875 * w, 0.5 * h)
                .c(w, 0.3 * h, 0.8 * w, 0.1 * h, 0.625 * w, 0.2 * h)
                .c(0.5 * w, 0.05 * h, 0.3 * w, 0.05 * h, 0.25 * w, 0.25 * h)
                .z();
            scene.push_path(p.done(), look.scene_style(0.0));
        }
        Kind::Document => {
            let dy = h * style.num("size", 0.3).clamp(0.0, 1.0);
            let fy = 1.4;
            let mut p = Pen::new(&xf);
            p.m(0.0, 0.0)
                .l(w, 0.0)
                .l(w, h - dy / 2.0)
                .q(w * 3.0 / 4.0, h - dy * fy, w / 2.0, h - dy / 2.0)
                .q(w / 4.0, h - dy * (1.0 - fy), 0.0, h - dy / 2.0)
                .z();
            scene.push_path(p.done(), look.scene_style(0.0));
        }
        Kind::Process => {
            draw_rect(scene, style, rect, look);
            let s = if style.flag("fixedSize", false) {
                style.num("size", 14.0).clamp(0.0, w / 2.0)
            } else {
                w * style.num("size", 0.1).clamp(0.0, 0.5)
            };
            let mut p = Pen::new(&xf);
            p.m(s, 0.0).l(s, h).m(w - s, 0.0).l(w - s, h);
            scene.push_path(p.done(), look.stroke_only());
        }
        Kind::Note => {
            let s = style.num("size", 30.0).clamp(0.0, w.min(h));
            let pts: Vec<Pt> =
                [(0.0, 0.0), (w - s, 0.0), (w, s), (w, h), (0.0, h)].iter().map(|&(x, y)| xf.p(x, y)).collect();
            scene.push_polygon(&pts, look.scene_style(0.0));
            let mut p = Pen::new(&xf);
            p.m(w - s, 0.0).l(w - s, s).l(w, s);
            scene.push_path(p.done(), look.stroke_only());
        }
        Kind::Card => {
            let s = style.num("size", 30.0).clamp(0.0, w.min(h));
            let pts: Vec<Pt> =
                [(s, 0.0), (w, 0.0), (w, h), (0.0, h), (0.0, s)].iter().map(|&(x, y)| xf.p(x, y)).collect();
            scene.push_polygon(&pts, look.scene_style(0.0));
        }
        Kind::Callout => {
            let s = style.num("size", 30.0).clamp(0.0, h);
            let base = style.num("base", 20.0);
            let x = w * style.num("position", 0.5).clamp(0.0, 1.0);
            let tip = w * style.num("position2", 0.5).clamp(0.0, 1.0);
            let pts: Vec<Pt> =
                [(0.0, 0.0), (w, 0.0), (w, h - s), ((x + base).min(w), h - s), (tip, h), (x, h - s), (0.0, h - s)]
                    .iter()
                    .map(|&(x, y)| xf.p(x, y))
                    .collect();
            scene.push_polygon(&pts, look.scene_style(0.0));
        }
        Kind::InternalStorage => {
            draw_rect(scene, style, rect, look);
            let (dx, dy) = (style.num("dx", 20.0).min(w), style.num("dy", 20.0).min(h));
            let mut p = Pen::new(&xf);
            p.m(dx, 0.0).l(dx, h).m(0.0, dy).l(w, dy);
            scene.push_path(p.done(), look.stroke_only());
        }
        Kind::Folder => {
            let tw = style.num("tabWidth", 60.0).min(w);
            let th = style.num("tabHeight", 20.0).min(h);
            let pts: Vec<Pt> = [(0.0, 0.0), (tw, 0.0), (tw + th * 0.5, th), (w, th), (w, h), (0.0, h)]
                .iter()
                .map(|&(x, y)| xf.p(x, y))
                .collect();
            scene.push_polygon(&pts, look.scene_style(0.0));
        }
        Kind::Cube => {
            let s = style.num("size", 20.0).clamp(0.0, w.min(h));
            let pts: Vec<Pt> = [(0.0, s), (s, 0.0), (w, 0.0), (w, h - s), (w - s, h), (0.0, h)]
                .iter()
                .map(|&(x, y)| xf.p(x, y))
                .collect();
            scene.push_polygon(&pts, look.scene_style(0.0));
            let mut p = Pen::new(&xf);
            p.m(0.0, s).l(w - s, s).l(w, 0.0).m(w - s, s).l(w - s, h);
            scene.push_path(p.done(), look.stroke_only());
        }
        Kind::Actor => {
            let mut p = Pen::new(&xf);
            p.m(0.0, h)
                .c(0.0, 3.0 * h / 5.0, 0.0, 2.0 * h / 5.0, w / 2.0, 2.0 * h / 5.0)
                .c(w / 2.0 - w / 6.0, 2.0 * h / 5.0, w / 2.0 - w / 6.0, 0.0, w / 2.0, 0.0)
                .c(w / 2.0 + w / 6.0, 0.0, w / 2.0 + w / 6.0, 2.0 * h / 5.0, w / 2.0, 2.0 * h / 5.0)
                .c(w, 2.0 * h / 5.0, w, 3.0 * h / 5.0, w, h)
                .z();
            scene.push_path(p.done(), look.scene_style(0.0));
        }
        Kind::UmlActor => {
            let hw = w * 2.0 / 6.0;
            let mut head = Pen::new(&xf);
            let (cx, cy, rx, ry) = (w / 2.0, h / 8.0, hw / 2.0, h / 8.0);
            head.m(cx + rx, cy).arc(cx, cy, rx, ry, 0.0, 2.0 * PI).z();
            scene.push_path(head.done(), look.scene_style(0.0));
            let mut p = Pen::new(&xf);
            p.m(w / 2.0, h / 4.0)
                .l(w / 2.0, 2.0 * h / 3.0)
                .m(w / 2.0, h / 3.0)
                .l(0.0, h / 3.0)
                .m(w / 2.0, h / 3.0)
                .l(w, h / 3.0)
                .m(w / 2.0, 2.0 * h / 3.0)
                .l(0.0, h)
                .m(w / 2.0, 2.0 * h / 3.0)
                .l(w, h);
            scene.push_path(p.done(), look.stroke_only());
        }
        Kind::Swimlane => label = draw_swimlane(scene, style, rect, look),
        Kind::Line => {
            let mut p = Pen::new(&xf);
            p.m(0.0, h / 2.0).l(w, h / 2.0);
            scene.push_path(p.done(), look.stroke_only());
        }
        Kind::Partial => draw_partial(scene, style, &xf, look),
        Kind::Lifeline => {
            let head = style.num("size", 40.0).clamp(0.0, rect.h);
            draw_rect(scene, style, Rect::new(rect.x, rect.y, rect.w, head), look);
            if let Some(st) = look.stroke.clone() {
                let pattern = vec![8.0 * st.width, 8.0 * st.width];
                scene.push_polyline(&[(rect.cx(), rect.y + head), (rect.cx(), rect.bottom())], st.dash(pattern));
            }
            label = Rect::new(rect.x, rect.y, rect.w, head);
        }
        Kind::Image => {
            scene.warn("images are not embedded; image cells are drawn as a placeholder");
            let gray = Paint::solid("#808080");
            let st = Stroke::new(gray.clone(), 1.0).dash(vec![3.0, 3.0]);
            scene.push_rect(
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                0.0,
                SceneStyle::new().fill(Paint::solid("#f2f2f2")).stroke(st.clone()),
            );
            scene.push_polyline(&[(rect.x, rect.y), (rect.right(), rect.bottom())], st.clone());
            scene.push_polyline(&[(rect.right(), rect.y), (rect.x, rect.bottom())], st);
        }
        Kind::Unknown(name) => {
            scene.warn(format!("shape \"{name}\" is not supported and is drawn as a dashed box"));
            let stroke = Stroke::new(Paint::solid("#808080"), 1.0).dash(vec![4.0, 3.0]);
            scene.push_rect(
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                0.0,
                SceneStyle::new().fill(Paint::solid("#f8f8f8")).stroke(stroke),
            );
        }
    }
    Drawn { label }
}

fn stroke_width(look: &Look) -> f32 {
    look.stroke.as_ref().map_or(0.0, |s| s.width)
}

fn draw_rect(scene: &mut Scene, style: &Style, rect: Rect, look: &Look) {
    let radius = if style.flag("rounded", false) {
        let arc = style.num("arcSize", 15.0);
        let r = if style.flag("absoluteArcSize", false) { arc } else { arc / 100.0 * rect.w.min(rect.h) };
        r.clamp(0.0, rect.w.min(rect.h) / 2.0)
    } else {
        0.0
    };
    scene.push_rect(rect.x, rect.y, rect.w, rect.h, radius, look.scene_style(style.num("rotation", 0.0)));
}

/// Returns the vertical radius of the end caps.
fn draw_cylinder(scene: &mut Scene, style: &Style, xf: &Xf, look: &Look) -> f32 {
    let (w, h) = (xf.lw(), xf.lh());
    // `cylinder3` carries an explicit size, the classic one derives it from the height.
    let ry = match style.opt_num("size") {
        Some(s) => s.clamp(0.0, h / 2.0),
        None => (h / 5.0).round().min(40.0) / 2.0,
    };
    let rx = w / 2.0;
    let mut body = Pen::new(xf);
    body.m(0.0, ry).arc(rx, ry, rx, ry, PI, 2.0 * PI).l(w, h - ry).arc(rx, h - ry, rx, ry, 0.0, PI).z();
    scene.push_path(body.done(), look.scene_style(0.0));
    let mut front = Pen::new(xf);
    front.m(0.0, ry).arc(rx, ry, rx, ry, PI, 0.0);
    scene.push_path(front.done(), look.stroke_only());
    ry
}

fn draw_datastore(scene: &mut Scene, xf: &Xf, look: &Look) {
    let (w, h) = (xf.lw(), xf.lh());
    let ry = (h / 8.0).min(w / 4.0);
    let rx = w / 2.0;
    let mut body = Pen::new(xf);
    body.m(0.0, ry).arc(rx, ry, rx, ry, PI, 2.0 * PI).l(w, h - ry).arc(rx, h - ry, rx, ry, 0.0, PI).z();
    scene.push_path(body.done(), look.scene_style(0.0));
    let mut bands = Pen::new(xf);
    for i in 0..3 {
        let y = ry + (h - 2.0 * ry) * i as f32 / 3.0;
        bands.m(0.0, y).arc(rx, y, rx, ry, PI, 0.0);
    }
    scene.push_path(bands.done(), look.stroke_only());
}

/// Returns the header area, where the title goes.
fn draw_swimlane(scene: &mut Scene, style: &Style, rect: Rect, look: &Look) -> Rect {
    let horizontal = style.flag("horizontal", true);
    let start = style.num("startSize", 23.0).clamp(0.0, if horizontal { rect.h } else { rect.w });
    let radius = if style.flag("rounded", false) {
        (style.num("arcSize", 15.0) / 100.0 * rect.w.min(rect.h)).min(start / 2.0)
    } else {
        0.0
    };
    let (header, body) = if horizontal {
        (Rect::new(rect.x, rect.y, rect.w, start), Rect::new(rect.x, rect.y + start, rect.w, rect.h - start))
    } else {
        (Rect::new(rect.x, rect.y, start, rect.h), Rect::new(rect.x + start, rect.y, rect.w - start, rect.h))
    };
    // The body is only filled when the style asks for it; the outline always covers
    // the whole container.
    let body_fill =
        style.color("swimlaneFillColor", None).and_then(|c| paint_with_gradient(style, c, "none", "fillOpacity"));
    let body_look = Look { fill: body_fill, ..look.clone() };
    if body.w > 0.0 && body.h > 0.0 {
        scene.push_rect(rect.x, rect.y, rect.w, rect.h, radius, body_look.scene_style(0.0));
    }
    if start > 0.0 {
        scene.push_rect(header.x, header.y, header.w, header.h, radius, look.scene_style(0.0));
    }
    header
}

fn draw_partial(scene: &mut Scene, style: &Style, xf: &Xf, look: &Look) {
    let (w, h) = (xf.lw(), xf.lh());
    let fill = Look { stroke: None, ..look.clone() };
    let corners: Vec<Pt> = [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)].iter().map(|&(x, y)| xf.p(x, y)).collect();
    if fill.fill.is_some() {
        scene.push_polygon(&corners, fill.fill_only());
    }
    let side = |key: &str| style.flag(key, true);
    let mut p = Pen::new(xf);
    if side("top") {
        p.m(0.0, 0.0).l(w, 0.0);
    }
    if side("right") {
        p.m(w, 0.0).l(w, h);
    }
    if side("bottom") {
        p.m(w, h).l(0.0, h);
    }
    if side("left") {
        p.m(0.0, h).l(0.0, 0.0);
    }
    scene.push_path(p.done(), look.stroke_only());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Geom, Item};

    fn near(a: Pt, b: Pt) -> bool {
        (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3
    }

    #[test]
    fn rect_perimeter_hits_sides() {
        let o = Outline::Rect(Rect::new(0.0, 0.0, 100.0, 50.0));
        assert!(near(o.hit_ray((50.0, 25.0), (300.0, 25.0)), (100.0, 25.0)));
        assert!(near(o.hit_ray((50.0, 25.0), (50.0, -80.0)), (50.0, 0.0)));
        assert!(near(o.hit_ray((50.0, 25.0), (150.0, 75.0)), (100.0, 50.0)));
    }

    #[test]
    fn ellipse_perimeter() {
        let o = Outline::Ellipse(Rect::new(0.0, 0.0, 100.0, 50.0));
        assert!(near(o.hit_ray((50.0, 25.0), (200.0, 25.0)), (100.0, 25.0)));
        assert!(near(o.hit_ray((50.0, 25.0), (50.0, 200.0)), (50.0, 50.0)));
        let p = o.hit_ray((50.0, 25.0), (150.0, 75.0));
        let n = ((p.0 - 50.0) / 50.0).powi(2) + ((p.1 - 25.0) / 25.0).powi(2);
        assert!((n - 1.0).abs() < 1e-3);
    }

    #[test]
    fn rhombus_perimeter() {
        let style = Style::parse("rhombus");
        let o = outline(&style, Rect::new(0.0, 0.0, 100.0, 60.0));
        assert!(near(o.hit_ray((50.0, 30.0), (400.0, 30.0)), (100.0, 30.0)));
        // Toward the lower right the ray leaves through the edge from (100,30) to (50,60).
        let p = o.hit_ray((50.0, 30.0), (200.0, 100.0));
        assert!((p.0 - 50.0) / 50.0 + (p.1 - 30.0) / 30.0 - 1.0 < 1e-3, "{p:?}");
    }

    #[test]
    fn triangle_faces_east_by_default_and_turns_with_direction() {
        let east = outline(&Style::parse("triangle"), Rect::new(0.0, 0.0, 100.0, 80.0));
        let Outline::Poly(p) = east else { panic!() };
        assert!(near(p[1], (100.0, 40.0)));
        let north = outline(&Style::parse("triangle;direction=north"), Rect::new(0.0, 0.0, 100.0, 80.0));
        let Outline::Poly(p) = north else { panic!() };
        assert!(p.iter().any(|&q| near(q, (50.0, 0.0))), "{p:?}");
    }

    #[test]
    fn orthogonal_perimeter_projects() {
        let o = Outline::Rect(Rect::new(0.0, 0.0, 100.0, 50.0));
        // The next point is level with the box, so the edge leaves horizontally at its height.
        assert!(near(o.hit_orthogonal((50.0, 25.0), (300.0, 40.0)), (100.0, 40.0)));
        assert!(near(o.hit_orthogonal((50.0, 25.0), (20.0, 300.0)), (20.0, 50.0)));
    }

    #[test]
    fn extents_on_lines() {
        let o = Outline::Ellipse(Rect::new(0.0, 0.0, 100.0, 100.0));
        let (a, b) = o.hline(50.0).unwrap();
        assert!((a - 0.0).abs() < 1e-3 && (b - 100.0).abs() < 1e-3);
        assert!(o.hline(120.0).is_none());
        let d = outline(&Style::parse("rhombus"), Rect::new(0.0, 0.0, 100.0, 100.0));
        let (a, b) = d.vline(50.0).unwrap();
        assert!((a - 0.0).abs() < 1e-3 && (b - 100.0).abs() < 1e-3);
        let (a, b) = d.vline(25.0).unwrap();
        assert!((a - 25.0).abs() < 1e-3 && (b - 75.0).abs() < 1e-3);
    }

    #[test]
    fn rounded_rect_radius_follows_arc_size() {
        let mut s = Scene::new(1.0, 1.0);
        let style = Style::parse("rounded=1;arcSize=50");
        draw(&mut s, &style, Rect::new(0.0, 0.0, 100.0, 40.0), &Look::of(&style));
        let Item::Shape(sh) = &s.items[0] else { panic!() };
        let Geom::Rect { radius, .. } = sh.geom else { panic!() };
        assert!((radius - 20.0).abs() < 1e-3);
        let style = Style::parse("rounded=1;absoluteArcSize=1;arcSize=6");
        let mut s = Scene::new(1.0, 1.0);
        draw(&mut s, &style, Rect::new(0.0, 0.0, 100.0, 40.0), &Look::of(&style));
        let Item::Shape(sh) = &s.items[0] else { panic!() };
        let Geom::Rect { radius, .. } = sh.geom else { panic!() };
        assert!((radius - 6.0).abs() < 1e-3);
    }

    #[test]
    fn zero_size_ellipse_perimeter_is_finite() {
        let o = Outline::Ellipse(Rect::new(10.0, 10.0, 0.0, 0.0));
        let p = o.hit_ray((10.0, 10.0), (50.0, 50.0));
        assert!(p.0.is_finite() && p.1.is_finite());
        let thin = Outline::Ellipse(Rect::new(0.0, 0.0, 100.0, 0.0));
        let p = thin.hit_ray((50.0, 0.0), (50.0, 50.0));
        assert!(p.0.is_finite() && p.1.is_finite());
    }

    #[test]
    fn dash_scales_with_stroke_width() {
        let s = Style::parse("dashed=1;strokeWidth=2");
        assert_eq!(dash_pattern(&s, 2.0), vec![6.0, 6.0]);
        let s = Style::parse("dashed=1;dashPattern=8 4 2");
        assert_eq!(dash_pattern(&s, 1.0), vec![8.0, 4.0, 2.0, 8.0, 4.0, 2.0]);
        let s = Style::parse("dashed=1;dashPattern=1e999 2");
        assert!(dash_pattern(&s, 1.0).iter().all(|v| v.is_finite()));
        let s = Style::parse("dashed=1;dashPattern=3e38 3e38");
        assert!(dash_pattern(&s, 1e10).iter().all(|v| v.is_finite()));
    }

    #[test]
    fn unknown_shapes_warn_and_draw_a_box() {
        let mut s = Scene::new(1.0, 1.0);
        let style = Style::parse("shape=mxgraph.aws4.lambda;fillColor=#f00");
        draw(&mut s, &style, Rect::new(0.0, 0.0, 10.0, 10.0), &Look::of(&style));
        assert_eq!(s.items.len(), 1);
        assert!(s.warnings[0].contains("mxgraph.aws4.lambda"));
    }

    #[test]
    fn gradient_directions() {
        let p = fill_paint(&Style::parse("fillColor=#fff;gradientColor=#000;gradientDirection=east")).unwrap();
        assert_eq!(p.gradient.unwrap().angle, 0.0);
        let p = fill_paint(&Style::parse("fillColor=#fff;gradientColor=#000")).unwrap();
        assert_eq!(p.gradient.unwrap().angle, 90.0);
    }
}
