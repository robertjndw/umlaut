//! Flowchart layout: every node is a shape sized from its label, subgraphs are clusters,
//! and `graph` does the placing, as for PlantUML's component diagrams.

use super::{Def, Model, Node, Shape};
use crate::mermaid::theme;
use crate::plantuml::diagrams::gcommon::GroupStyle;
use crate::plantuml::layout::gdraw::{self, cylinder, draw_group, graph_opts, group_metrics, title_centered, GraphInfo, GroupLook, Slot};
use crate::plantuml::layout::graph::{self, Arrange, Border, Cluster, Graph, Node as GNode, Rect};
use crate::plantuml::layout::shapes::{dashed, solid};
use crate::plantuml::layout::Body;
use crate::plantuml::model::Measured;
use crate::plantuml::skin::Skin;
use crate::scene::{HAlign, Paint, Scene, Stroke, Style};

/// Space between a label and the outline of a rectangle. Mermaid pads 15 px on every
/// side, but its label boxes are taller than the glyphs Typst measures.
const PAD_X: f32 = 15.0;
const PAD_Y: f32 = 12.0;
/// Half the height of a cylinder's lids.
const LID: f32 = 7.0;
/// Gap between the two rings of a double circle.
const RING: f32 = 5.0;
/// Radius of the rounded corners of `A(text)`.
const ROUND: f32 = 5.0;
/// Distance of a subroutine's inner lines from its sides.
const FRAME: f32 = 8.0;

/// How far the sides of a hexagon, parallelogram or trapezoid lean, from the height.
fn lean_of(h: f32) -> f32 {
    (h / 4.0).round()
}

/// Size and outline of a node whose label measures `tw` by `th`.
fn metrics(shape: Shape, (tw, th): (f32, f32)) -> (f32, f32, Border) {
    let (w, h) = (tw + 2.0 * PAD_X, th + 2.0 * PAD_Y);
    let lean = lean_of(h);
    let slanted = |w: f32| (w, h, Border::Slanted(lean as u16));
    match shape {
        Shape::Rect | Shape::Round => (w, h, Border::Rect),
        Shape::Stadium => (w + h / 4.0, h, Border::Rect),
        Shape::Subroutine => (w + 2.0 * FRAME, h, Border::Rect),
        Shape::Cylinder => (w, h + 2.0 * LID, Border::Rect),
        Shape::Circle => {
            let d = (tw.max(th) + 2.0 * PAD_X).max(h);
            (d, d, Border::Ellipse)
        }
        Shape::DoubleCircle => {
            let d = (tw.max(th) + 2.0 * PAD_X).max(h) + 2.0 * RING;
            (d, d, Border::Ellipse)
        }
        // The diamond is square, as in Mermaid; the label rectangle w by h then fits
        // exactly in the diamond whatever its proportions.
        Shape::Rhombus => (w + h, w + h, Border::Diamond),
        Shape::Hexagon => slanted(w + 2.0 * lean),
        Shape::LeanRight | Shape::LeanLeft => slanted(w + lean),
        Shape::Trapezoid | Shape::TrapezoidAlt => slanted(w + 2.0 * lean),
        Shape::Asymmetric => slanted(w + lean),
    }
}

pub fn layout(model: &Model, skin: &Skin, m: &Measured) -> Result<Body, String> {
    layout_info(model, skin, m).map(|r| r.0)
}

pub fn layout_info(model: &Model, skin: &Skin, m: &Measured) -> Result<(Body, GraphInfo), String> {
    let mut g = Graph::default();
    // A node can be mentioned at the top level and put into a subgraph later, so a
    // cluster can come after its members: hand out all slots first, then fill them in.
    let mut slots: Vec<Slot> = Vec::new();
    let (mut nc, mut nn) = (0, 0);
    for e in &model.ents {
        match e.def {
            Def::Subgraph(_) => {
                slots.push(Slot::Cluster(nc));
                nc += 1;
            }
            Def::Node(_) => {
                slots.push(Slot::Node(nn));
                nn += 1;
            }
        }
    }
    g.clusters = vec![Cluster { parent: None, inset: [0.0; 4], min: (0.0, 0.0), arrange: Arrange::Graph, title_centered: true }; nc];
    g.nodes = vec![GNode { w: 0.0, h: 0.0, parent: None, border: Border::Rect }; nn];
    let size_of = |label| m.size(label);
    for (e, slot) in model.ents.iter().zip(&slots) {
        let parent = e.parent.and_then(|p| slots[p].cluster());
        match (&e.def, slot) {
            (Def::Subgraph(s), Slot::Cluster(c)) => {
                let (inset, min) = group_metrics(GroupStyle::Rectangle, s.title.map(size_of));
                g.clusters[*c] = Cluster { parent, inset, min, arrange: Arrange::Graph, title_centered: title_centered(GroupStyle::Rectangle) };
            }
            (Def::Node(n), Slot::Node(i)) => {
                let (w, h, border) = metrics(n.shape, n.label.map_or((0.0, 0.0), size_of));
                g.nodes[*i] = GNode { w, h, parent, border };
            }
            _ => {}
        }
    }
    let edge_of_link = gdraw::build_edges(&mut g, &slots, &model.links, m, false);
    let lay = graph::layout(&g, &graph_opts(skin, model.lr, model.reverse))?;

    let mut body = Scene::new(0.0, 0.0);
    let mut titles = Vec::new();

    // Outer subgraphs first, so inner ones are drawn over them.
    let depth = |mut i: usize| {
        let mut d = 0;
        while let Some(p) = model.ents[i].parent {
            d += 1;
            i = p;
        }
        d
    };
    let mut order: Vec<usize> = (0..model.ents.len()).filter(|&i| matches!(slots[i], Slot::Cluster(_))).collect();
    order.sort_by_key(|&i| (depth(i), i));
    for i in order {
        let (Def::Subgraph(s), Slot::Cluster(c)) = (&model.ents[i].def, &slots[i]) else { continue };
        let fill = s.css.fill.as_deref().map_or_else(|| skin.fill(&["package"], theme::CLUSTER_FILL), Paint::solid);
        let look = GroupLook {
            fill: Some(fill.clone()),
            tab_fill: fill,
            border: s.css.stroke.clone().unwrap_or_else(|| skin.border(&["package"], theme::BORDER2)),
            width: s.css.stroke_width.unwrap_or(1.0),
            dashed: s.css.dash.is_some(),
        };
        let dx = s.title.map_or(0.0, |t| lay.title_dx(&lay.clusters[*c], m.size(t)));
        if let Some(t) = draw_group(&mut body, m, GroupStyle::Rectangle, &lay.clusters[*c], s.title, dx, &look) {
            titles.push((i, t));
        }
    }

    let arrow_color = skin.color(&["arrowcolor"], theme::LINE);
    gdraw::draw_links(&mut body, &lay, &model.links, &edge_of_link, |_| false, &arrow_color, false, skin, m);
    for (e, slot) in model.ents.iter().zip(&slots) {
        if let (Def::Node(n), Slot::Node(i)) = (&e.def, slot) {
            draw_node(&mut body, n, &lay.nodes[*i], m, skin);
        }
    }

    let (mut out, _, _) = gdraw::finish(body);
    let mut info = gdraw::collect_info(&lay, &g, &slots, &edge_of_link, titles);
    let names = model.ents.iter().map(|e| if let Def::Node(Node { label: Some(l), .. }) = &e.def { m.plain(*l) } else { e.key.clone() }).collect();
    info.report(&mut out.scene, names, model.ents.iter().map(|e| e.parent).collect(), &model.links, m);
    Ok((out, info))
}

fn draw_node(scene: &mut Scene, n: &Node, r: &Rect, m: &Measured, skin: &Skin) {
    let css = &n.css;
    let fill = css.fill.as_deref().map_or_else(|| skin.fill(&["node"], theme::PRIMARY), Paint::solid);
    let border = css.stroke.clone().unwrap_or_else(|| skin.border(&["node"], theme::BORDER));
    let width = css.stroke_width.unwrap_or(1.0);
    let stroke = || -> Stroke {
        match &css.dash {
            Some(d) => dashed(&border, width, &[d[0], d.get(1).copied().unwrap_or(d[0])]),
            None => solid(&border, width),
        }
    };
    let st = || Style::new().fill(fill.clone()).stroke(stroke());
    let (cx, cy) = r.center();
    let (x, y, right, bottom) = (r.x, r.y, r.right(), r.bottom());
    let lean = lean_of(r.h);
    let mut text_x = cx;
    let mut text_y = cy;
    match n.shape {
        Shape::Rect => scene.push_rect(x, y, r.w, r.h, 0.0, st()),
        Shape::Round => scene.push_rect(x, y, r.w, r.h, skin.radius(&["node"], ROUND * 2.0), st()),
        Shape::Stadium => scene.push_rect(x, y, r.w, r.h, r.h / 2.0, st()),
        Shape::Subroutine => {
            scene.push_rect(x, y, r.w, r.h, 0.0, st());
            for lx in [x + FRAME, right - FRAME] {
                scene.push_polyline(&[(lx, y), (lx, bottom)], stroke());
            }
        }
        Shape::Cylinder => {
            cylinder(scene, r, LID, fill.clone(), &border, width);
            text_y += LID;
        }
        Shape::Circle => scene.push_ellipse(x, y, r.w, r.h, st()),
        Shape::DoubleCircle => {
            scene.push_ellipse(x, y, r.w, r.h, st());
            scene.push_ellipse(x + RING, y + RING, r.w - 2.0 * RING, r.h - 2.0 * RING, st());
        }
        Shape::Asymmetric => {
            scene.push_polygon(&[(x, y), (right, y), (right, bottom), (x, bottom), (x + lean, cy)], st());
            text_x += lean / 2.0;
        }
        Shape::Rhombus => scene.push_polygon(&[(cx, y), (right, cy), (cx, bottom), (x, cy)], st()),
        Shape::Hexagon => scene.push_polygon(&[(x + lean, y), (right - lean, y), (right, cy), (right - lean, bottom), (x + lean, bottom), (x, cy)], st()),
        Shape::LeanRight => scene.push_polygon(&[(x + lean, y), (right, y), (right - lean, bottom), (x, bottom)], st()),
        Shape::LeanLeft => scene.push_polygon(&[(x, y), (right - lean, y), (right, bottom), (x + lean, bottom)], st()),
        Shape::Trapezoid => scene.push_polygon(&[(x + lean, y), (right - lean, y), (right, bottom), (x, bottom)], st()),
        Shape::TrapezoidAlt => scene.push_polygon(&[(x, y), (right, y), (right - lean, bottom), (x + lean, bottom)], st()),
    }
    if let Some(l) = n.label {
        let (_, th) = m.size(l);
        m.text_lines_at(scene, l, text_x, text_y - th / 2.0, HAlign::Center, HAlign::Center);
    }
}
