//! draw.io (`.drawio`, `.drawio.svg`) to scene.
//!
//! Everything is drawn in draw.io pixels, which map 1:1 to pt like draw.io's own PDF
//! export. Cells are visited depth first in document order, so the stacking matches
//! what draw.io shows.

mod decode;
mod edges;
mod geometry;
mod label;
mod shapes;
mod style;

use std::collections::HashMap;
use std::rc::Rc;

use crate::scene::{HAlign, Paint, Run, Scene, Stroke, Style as SceneStyle, TextBox, VAlign};
use crate::DrawioOptions;

use edges::Terminal;
use geometry::{Model, Pt, Rect};
use label::TextProps;
use style::Style;

/// Space around the drawing so strokes at the edge are not cut off.
const MARGIN: f32 = 4.0;

pub fn render(source: &[u8], opts: &DrawioOptions) -> Result<Scene, String> {
    let xml = decode::decode(source, &opts.page)?;
    let model = Model::parse(&xml)?;
    let mut r =
        Renderer { model: &model, scene: Scene::new(0.0, 0.0), terminals: HashMap::new(), routes: HashMap::new() };
    for &root in &model.roots {
        r.visit(root);
    }
    Ok(r.finish())
}

struct Renderer<'a> {
    model: &'a Model,
    scene: Scene,
    // Shared rather than cloned: an edge looks up both of its ends, and an outline can
    // be a whole polygon.
    terminals: HashMap<usize, Option<Rc<Terminal<'a>>>>,
    routes: HashMap<usize, Vec<Pt>>,
}

impl<'a> Renderer<'a> {
    fn visit(&mut self, idx: usize) {
        let cell = &self.model.cells[idx];
        if !cell.visible {
            return;
        }
        let parent_is_edge = cell.parent.is_some_and(|p| self.model.cells[p].edge);
        if cell.edge {
            self.draw_edge(idx);
        } else if cell.vertex {
            if parent_is_edge {
                self.draw_edge_child(idx);
            } else {
                self.draw_vertex(idx);
            }
        }
        if cell.collapsed {
            return;
        }
        for &child in &cell.children {
            self.visit(child);
        }
    }

    fn terminal(&mut self, idx: usize) -> Option<Rc<Terminal<'a>>> {
        if let Some(t) = self.terminals.get(&idx) {
            return t.clone();
        }
        let model = self.model;
        let cell = &model.cells[idx];
        let t = if cell.vertex { model.vertex_rect(idx).map(|r| Rc::new(Terminal::new(&cell.style, r))) } else { None };
        self.terminals.insert(idx, t.clone());
        t
    }

    fn runs_for(&self, value: &str, style: &Style) -> Vec<Run> {
        if value.is_empty() {
            return Vec::new();
        }
        let props = TextProps::from_style(style);
        if style.flag("html", false) {
            label::html_runs(value, &props)
        } else {
            label::plain_runs(value, &props)
        }
    }

    // Vertices ----------------------------------------------------------------------

    fn draw_vertex(&mut self, idx: usize) {
        let model = self.model;
        let cell = &model.cells[idx];
        let Some(rect) = model.vertex_rect(idx) else { return };
        let style = &cell.style;
        let runs = self.runs_for(&cell.value, style);

        let mut area = rect;
        if rect.w > 0.0 || rect.h > 0.0 {
            let look = shapes::Look::of(style);
            area = shapes::draw(&mut self.scene, style, rect, &look).label;
            warn_sketch(&mut self.scene, style);
        }
        if !label::is_blank(&runs) {
            place_label(&mut self.scene, style, runs, rect, area);
        }
    }

    // Edges -------------------------------------------------------------------------

    fn draw_edge(&mut self, idx: usize) {
        let model = self.model;
        let cell = &model.cells[idx];
        let source = cell.source.as_deref().and_then(|id| model.find(id)).and_then(|i| self.terminal(i));
        let target = cell.target.as_deref().and_then(|id| model.find(id)).and_then(|i| self.terminal(i));
        let geo = cell.geo.clone().unwrap_or_default();
        let origin = cell.parent.map_or((0.0, 0.0), |p| model.child_origin(p));
        let shift = |p: Pt| (p.0 + origin.0, p.1 + origin.1);
        let waypoints: Vec<Pt> = geo.points.iter().map(|&p| shift(p)).collect();
        let spec = edges::EdgeSpec {
            style: &cell.style,
            source: source.as_deref(),
            target: target.as_deref(),
            waypoints: &waypoints,
            source_point: geo.source_point.map(shift),
            target_point: geo.target_point.map(shift),
            self_loop: cell.source.is_some() && cell.source == cell.target,
        };
        let mut warnings = Vec::new();
        let route = edges::route(&spec, &mut warnings);
        for w in warnings {
            self.scene.warn(w);
        }
        if route.len() < 2 {
            return;
        }
        edges::draw(&mut self.scene, &cell.style, &route);
        warn_sketch(&mut self.scene, &cell.style);

        let runs = self.runs_for(&cell.value, &cell.style);
        if !label::is_blank(&runs) {
            let at = edges::point_at(&route, geo.x, geo.y);
            let off = geo.offset.unwrap_or((0.0, 0.0));
            place_edge_label(&mut self.scene, &cell.style, runs, (at.0 + off.0, at.1 + off.1), 0.0);
        }
        self.routes.insert(idx, route);
    }

    /// Label cells attached to an edge: positioned along the edge instead of by
    /// absolute coordinates, and never drawn as shapes.
    fn draw_edge_child(&mut self, idx: usize) {
        let cell = &self.model.cells[idx];
        let Some(parent) = cell.parent else { return };
        let Some(route) = self.routes.get(&parent) else { return };
        let geo = cell.geo.clone().unwrap_or_default();
        let runs = self.runs_for(&cell.value, &cell.style);
        if label::is_blank(&runs) {
            return;
        }
        let at = edges::point_at(route, geo.x, geo.y);
        let off = geo.offset.unwrap_or((0.0, 0.0));
        place_edge_label(&mut self.scene, &cell.style, runs, (at.0 + off.0, at.1 + off.1), geo.w);
    }

    fn finish(mut self) -> Scene {
        let Some(b) = geometry::content_bounds(&self.scene) else {
            self.scene.size = [2.0 * MARGIN, 2.0 * MARGIN];
            self.scene.warn("the diagram is empty");
            return self.scene;
        };
        self.scene.translate(MARGIN - b.x, MARGIN - b.y);
        let size = [b.w + 2.0 * MARGIN, b.h + 2.0 * MARGIN];
        self.scene.size = size;
        if let Some(bg) = self.model.background.as_deref().and_then(|c| style::parse_color(c, None)) {
            let page = crate::scene::Item::Shape(crate::scene::Shape {
                geom: crate::scene::Geom::Rect { x: 0.0, y: 0.0, w: size[0], h: size[1], radius: 0.0 },
                bbox: [0.0, 0.0, size[0], size[1]],
                fill: Some(Paint::solid(&bg)),
                stroke: None,
                opacity: 1.0,
                rotation: 0.0,
            });
            self.scene.items.insert(0, page);
        }
        self.scene
    }
}

fn warn_sketch(scene: &mut Scene, style: &Style) {
    if style.flag("sketch", false) {
        scene.warn("sketch style is not supported; shapes are drawn clean");
    }
}

// Labels ----------------------------------------------------------------------------------

fn halign(s: &str) -> HAlign {
    match s {
        "left" => HAlign::Left,
        "right" => HAlign::Right,
        _ => HAlign::Center,
    }
}

fn valign(s: &str) -> VAlign {
    match s {
        "top" => VAlign::Top,
        "bottom" => VAlign::Bottom,
        _ => VAlign::Middle,
    }
}

/// The box around the text when it has a background or border: as wide as the text
/// rather than the label area. The estimate runs a little wide on purpose, the real
/// font is not Helvetica.
fn slack(width: f32) -> f32 {
    width * 1.08 + 2.0
}

/// `labelBackgroundColor=default` means the page background, which is white.
fn label_background(style: &Style, absent: Option<&str>) -> Option<String> {
    match style.get("labelBackgroundColor") {
        Some("default") => Some(crate::scene::hex("#ffffff")),
        _ => style.color("labelBackgroundColor", absent),
    }
}

/// Draws the label border and gives the text its background. `default_bg` is the
/// background when the style sets none.
fn label_extras(scene: &mut Scene, style: &Style, tb: TextBox, cw_rotation: f32, default_bg: Option<&str>) -> TextBox {
    let mut tb = tb;
    if let Some(border) = style.color("labelBorderColor", None) {
        scene.push_rect(
            tb.x,
            tb.y,
            tb.w,
            tb.h,
            0.0,
            SceneStyle::new()
                .stroke(Stroke::new(Paint::solid(&border), 1.0))
                .rotation(shapes::scene_rotation(cw_rotation)),
        );
    }
    if let Some(bg) = label_background(style, default_bg) {
        tb = tb.background(Paint::solid(&bg));
    }
    tb
}

/// Space between the label and its box: (left, right, top, bottom).
fn label_spacing(style: &Style) -> (f32, f32, f32, f32) {
    let spacing = style.num("spacing", 2.0);
    (
        spacing + style.num("spacingLeft", 0.0),
        spacing + style.num("spacingRight", 0.0),
        spacing + style.num("spacingTop", 0.0),
        spacing + style.num("spacingBottom", 0.0),
    )
}

/// Places the label of a vertex. `shape` is the cell's bounds, `area` where the label
/// goes by default (the whole cell, or the header of a swimlane).
fn place_label(scene: &mut Scene, style: &Style, runs: Vec<Run>, shape: Rect, area: Rect) {
    let (sl, sr, st, sb) = label_spacing(style);
    let label_pos = style.str_or("labelPosition", "center");
    let vlabel_pos = style.str_or("verticalLabelPosition", "middle");
    let align = style.str_or("align", "center");
    let valign_s = style.str_or("verticalAlign", "middle");
    let wrap = style.get("whiteSpace") == Some("wrap");
    let vertical_text = !style.flag("horizontal", true);
    let rotation = style.num("rotation", 0.0);

    // Text that runs vertically is laid out in a frame with the sides swapped.
    let frame = if vertical_text {
        let c = area.center();
        Rect::new(c.0 - area.h / 2.0, c.1 - area.w / 2.0, area.h, area.w)
    } else {
        area
    };
    let has_box =
        label_background(style, None).is_some() || style.color("labelBorderColor", None).is_some();

    let inner_w = (frame.w - sl - sr).max(0.0);
    let measured_w = if wrap && label_pos == "center" {
        Some(inner_w.max(1.0))
    } else if wrap {
        Some(frame.w.max(1.0))
    } else {
        None
    };
    let (tw, th) = label::measure(&runs, measured_w);
    let tw = slack(tw);

    let (mut bx, mut bw, ha);
    match label_pos {
        "left" => {
            bw = if wrap { frame.w } else { tw };
            bx = frame.x - sr - bw;
            ha = HAlign::Right;
        }
        "right" => {
            bw = if wrap { frame.w } else { tw };
            bx = frame.right() + sl;
            ha = HAlign::Left;
        }
        _ => {
            if wrap {
                bx = frame.x + sl;
                bw = inner_w;
            } else {
                bw = inner_w.max(tw);
                bx = match align {
                    "left" => frame.x + sl,
                    "right" => frame.x + sl + inner_w - bw,
                    _ => frame.x + sl + inner_w / 2.0 - bw / 2.0,
                };
            }
            ha = halign(align);
        }
    }

    let (mut by, mut bh, va);
    let text_h = th + 2.0;
    match vlabel_pos {
        "top" => {
            bh = text_h;
            by = frame.y - sb - bh;
            va = VAlign::Bottom;
        }
        "bottom" => {
            bh = text_h;
            by = frame.bottom() + st;
            va = VAlign::Top;
        }
        _ => {
            let inner_h = (frame.h - st - sb).max(0.0);
            bh = inner_h.max(text_h);
            by = match valign_s {
                "top" => frame.y + st,
                "bottom" => frame.y + st + inner_h - bh,
                _ => frame.y + st + inner_h / 2.0 - bh / 2.0,
            };
            va = valign(valign_s);
        }
    }

    if has_box {
        let hug_w = (tw + 4.0).min(bw);
        bx = match ha {
            HAlign::Left => bx,
            HAlign::Right => bx + bw - hug_w,
            HAlign::Center => bx + (bw - hug_w) / 2.0,
        };
        bw = hug_w;
        let hug_h = text_h.min(bh);
        by = match va {
            VAlign::Top => by,
            VAlign::Bottom => by + bh - hug_h,
            VAlign::Middle => by + (bh - hug_h) / 2.0,
        };
        bh = hug_h;
    }

    let center = geometry::rotate_about((bx + bw / 2.0, by + bh / 2.0), shape.center(), rotation);
    let text_rotation = rotation + if vertical_text { -90.0 } else { 0.0 };
    let tb = TextBox::new(center.0 - bw / 2.0, center.1 - bh / 2.0, bw, bh, runs)
        .align(ha, va)
        .wrap(wrap)
        .rotation(shapes::scene_rotation(text_rotation));
    let tb = label_extras(scene, style, tb, text_rotation, None);
    scene.push_text(tb);
}

/// Places an edge label centered on `at` (or hanging off it per `align` and
/// `verticalAlign`). Edge labels sit on a white background unless the style says
/// otherwise, like in draw.io.
fn place_edge_label(scene: &mut Scene, style: &Style, runs: Vec<Run>, at: Pt, wrap_width: f32) {
    let (sl, sr, st, sb) = label_spacing(style);
    let wrap = style.get("whiteSpace") == Some("wrap") && wrap_width > 0.0;
    let (tw, th) = label::measure(&runs, wrap.then_some(wrap_width));
    let bw = if wrap { wrap_width } else { slack(tw) } + sl + sr;
    let bh = th + 2.0 + st + sb;
    let align = style.str_or("align", "center");
    let valign_s = style.str_or("verticalAlign", "middle");
    let bx = match align {
        "left" => at.0,
        "right" => at.0 - bw,
        _ => at.0 - bw / 2.0,
    };
    let by = match valign_s {
        "top" => at.1,
        "bottom" => at.1 - bh,
        _ => at.1 - bh / 2.0,
    };
    let tb = TextBox::new(bx + sl, by + st, bw - sl - sr, bh - st - sb, runs)
        .align(halign(align), VAlign::Middle)
        .wrap(wrap)
        .rotation(shapes::scene_rotation(if style.flag("horizontal", true) { 0.0 } else { -90.0 }));
    // The border ignores `horizontal`, so it is drawn unrotated.
    let tb = label_extras(scene, style, tb, 0.0, Some("#ffffff"));
    scene.push_text(tb);
}

#[cfg(test)]
mod tests;
