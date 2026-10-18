//! Use case, component and deployment layout: every element is a box (or an ellipse, a
//! stick figure, a circle) sized from its label, containers are clusters, and
//! `graph` does the placing. The shapes follow PlantUML's: uml2 components with the
//! two-tab icon, 3D nodes, cylinders for databases, clouds, folders, frames.

use super::gdraw::{self, box3d, cloud_path, cylinder, draw_group, draw_note, graph_opts, group_metrics, note_size, title_centered, GraphInfo, GroupLook, Slot};
use super::graph::{self, Arrange, Border, Cluster, Graph, Node, Rect};
use super::shapes::{draw_icon, icon_size, note_shape, solid, IconKind, NoteKind};
use super::Body;
use crate::plantuml::diagrams::description::*;
use crate::plantuml::model::{is_blank, Measured};
use crate::plantuml::skin::{self, Skin};
use crate::scene::{HAlign, Paint, Scene, Style};

/// Name and stereotype stacked, as one block of text.
fn text_block(e: &Elem, m: &Measured) -> (f32, f32, f32) {
    let (nw, nh) = m.size(e.name);
    let (sw, sh) = e.stereo.map_or((0.0, 0.0), |s| m.size(s));
    // Measured text is as tall as its glyphs, so stacked lines need a little air.
    let gap = if e.stereo.is_some() { 4.0 } else { 0.0 };
    (nw.max(sw), nh + sh + gap, sh + gap)
}

fn icon_kind(k: DKind) -> Option<IconKind> {
    match k {
        DKind::Actor => Some(IconKind::Actor),
        DKind::Boundary => Some(IconKind::Boundary),
        DKind::Control => Some(IconKind::Control),
        DKind::Entity => Some(IconKind::Entity),
        _ => None,
    }
}

fn leaf_size(e: &Elem, m: &Measured) -> (f32, f32, Border) {
    let (tw, th, _) = text_block(e, m);
    if let Some(ik) = icon_kind(e.kind) {
        let (iw, ih) = icon_size(ik);
        return (iw.max(tw) + 8.0, ih + 12.0 + th, Border::Icon(iw as u16, ih as u16));
    }
    match e.kind {
        DKind::UseCase => ((tw + 16.0) * 1.4 + 6.0, ((th + 12.0) * 1.5).max(40.0), Border::Ellipse),
        DKind::Interface => ((18.0f32).max(tw) + 4.0, 18.0 + 4.0 + th, Border::Icon(18, 18)),
        DKind::Port | DKind::PortIn | DKind::PortOut => (12.0f32.max(tw), 12.0 + 4.0 + th, Border::Rect),
        DKind::Component => (tw + 58.0, th + 30.0, Border::Rect),
        DKind::Node => (tw + 44.0, th + 36.0, Border::Rect),
        DKind::Database => (tw + 30.0, th + 50.0, Border::Rect),
        DKind::Cloud => ((tw + 40.0) * 1.15, (th + 30.0) * 1.4, Border::Rect),
        DKind::Package | DKind::Folder | DKind::Frame => {
            let (_, min) = group_metrics(e.kind.group_style(), Some((tw, th)));
            (min.0.max(tw + 30.0), min.1 + 6.0, Border::Rect)
        }
        DKind::Hexagon => (tw + 50.0, th + 28.0, Border::Rect),
        DKind::Artifact => (tw + 44.0, th + 30.0, Border::Rect),
        _ => (tw + 30.0, th + 26.0, Border::Rect),
    }
}

pub fn layout(model: &Model, skin: &Skin, m: &Measured) -> Result<Body, String> {
    layout_info(model, skin, m).map(|r| r.0)
}

pub fn layout_info(model: &Model, skin: &Skin, m: &Measured) -> Result<(Body, GraphInfo), String> {
    let mut has_children = vec![false; model.ents.len()];
    for p in model.ents.iter().filter_map(|e| e.parent) {
        has_children[p] = true;
    }
    let mut g = Graph::default();
    let mut slots: Vec<Slot> = Vec::new();
    for (i, e) in model.ents.iter().enumerate() {
        let parent = e.parent.and_then(|p| slots[p].cluster());
        match &e.def {
            Def::Elem(el) if has_children[i] => {
                let title = (!is_blank(&m.label(el.name).runs)).then(|| m.size(el.name));
                let (inset, min) = group_metrics(el.kind.group_style(), title);
                g.clusters.push(Cluster { parent, inset, min, arrange: Arrange::Graph, title_centered: title_centered(el.kind.group_style()) });
                slots.push(Slot::Cluster(g.clusters.len() - 1));
            }
            Def::Elem(el) => {
                let (w, h, border) = leaf_size(el, m);
                g.nodes.push(Node { w, h, parent, border });
                slots.push(Slot::Node(g.nodes.len() - 1));
            }
            Def::Note(nt) => {
                if nt.on_link.is_some() {
                    slots.push(Slot::Floating);
                    continue;
                }
                let (w, h) = note_size(m, nt.label);
                g.nodes.push(Node { w, h, parent, border: Border::Rect });
                slots.push(Slot::Node(g.nodes.len() - 1));
            }
        }
    }
    let edge_of_link = gdraw::build_edges(&mut g, &slots, &model.links, m, false);
    let opts = graph_opts(skin, model.lr, false);
    let lay = graph::layout(&g, &opts)?;

    let uml2 = !skin.raw("componentstyle").is_some_and(|v| v.eq_ignore_ascii_case("rectangle"));
    let arrow_color = skin.color(&["arrowcolor"], skin::BORDER);
    let mut body = Scene::new(0.0, 0.0);
    let mut titles = Vec::new();

    for (i, e) in model.ents.iter().enumerate() {
        let (Def::Elem(el), Slot::Cluster(c)) = (&e.def, &slots[i]) else { continue };
        let pre = [el.kind.prefix()];
        let look = GroupLook {
            fill: el.look.fill.clone(),
            tab_fill: skin.fill(&pre, skin::GROUP_TAB_FILL),
            border: el.look.line.clone().unwrap_or_else(|| skin.border(&pre, skin::BORDER)),
            width: 1.0,
            dashed: el.look.dashed,
        };
        let title = Some(el.name).filter(|t| !is_blank(&m.label(*t).runs));
        let dx = title.map_or(0.0, |t| lay.title_dx(&lay.clusters[*c], m.size(t)));
        if let Some(t) = draw_group(&mut body, m, el.kind.group_style(), &lay.clusters[*c], title, dx, &look) {
            titles.push((i, t));
        }
    }
    let is_note = |i: usize| matches!(model.ents[i].def, Def::Note(_));
    gdraw::draw_links(&mut body, &lay, &model.links, &edge_of_link, is_note, &arrow_color, false, skin, m);
    let mut floating: Vec<Rect> = Vec::new();
    for (e, slot) in model.ents.iter().zip(&slots) {
        match (&e.def, slot) {
            (Def::Elem(el), Slot::Node(i)) => draw_leaf(&mut body, el, &lay.nodes[*i], m, skin, uml2),
            (Def::Note(nt), Slot::Node(i)) => draw_note(&mut body, m, nt.label, &lay.nodes[*i], nt.look.fill.clone(), skin),
            (Def::Note(nt), Slot::Floating) => gdraw::draw_link_note(&mut body, &lay, &edge_of_link, nt.on_link, nt.label, nt.look.fill.clone(), &mut floating, skin, m),
            _ => {}
        }
    }

    let (mut out, _, _) = gdraw::finish(body);
    let mut info = gdraw::collect_info(&lay, &g, &slots, &edge_of_link, titles);
    let names = model.ents.iter().map(|e| if e.key.is_empty() && matches!(e.def, Def::Note(_)) { "a note".to_string() } else { e.key.clone() }).collect();
    info.report(&mut out.scene, names, model.ents.iter().map(|e| e.parent).collect(), &model.links, m);
    Ok((out, info))
}

fn draw_leaf(scene: &mut Scene, e: &Elem, r: &Rect, m: &Measured, skin: &Skin, uml2: bool) {
    let pre = [e.kind.prefix()];
    let fill = e.look.fill.clone().unwrap_or_else(|| skin.fill(&pre, skin::ACTIVITY_FILL));
    let border = e.look.line.clone().unwrap_or_else(|| skin.border(&pre, skin::BORDER));
    let bw = if e.look.bold { 2.0 } else { 1.0 };
    let st = || {
        let s = Style::new().fill(fill.clone());
        s.stroke(gdraw::outline(&border, bw, e.look.dashed))
    };
    let (_, th, sh) = text_block(e, m);
    let (cx, cy) = r.center();
    // Centered name block with the stereotype above it.
    let centered = |scene: &mut Scene, cx: f32, top: f32| {
        if let Some(s) = e.stereo {
            m.text_at(scene, s, cx, top, HAlign::Center);
        }
        m.text_at(scene, e.name, cx, top + sh, HAlign::Center);
    };
    if let Some(ik) = icon_kind(e.kind) {
        draw_icon(scene, ik, cx, r.y + 4.0, &fill, &border, bw);
        centered(scene, cx, r.bottom() - 4.0 - th);
        return;
    }
    match e.kind {
        DKind::UseCase => {
            scene.push_ellipse(r.x, r.y, r.w, r.h, st());
            centered(scene, cx, cy - th / 2.0);
            if e.business {
                scene.push_polyline(&[(r.x + r.w * 0.2, r.y + r.h * 0.2), (r.x + r.w * 0.33, r.y + r.h * 0.07)], solid(&border, bw));
            }
        }
        DKind::Interface => {
            scene.push_ellipse(cx - 9.0, r.y, 18.0, 18.0, st());
            centered(scene, cx, r.y + 22.0);
        }
        DKind::Port | DKind::PortIn | DKind::PortOut => {
            scene.push_rect(cx - 6.0, r.y, 12.0, 12.0, 0.0, st());
            centered(scene, cx, r.y + 16.0);
        }
        DKind::Component => {
            scene.push_rect(r.x, r.y, r.w, r.h, 0.0, st());
            if uml2 {
                let (ix, iy) = (r.right() - 22.0, r.y + 6.0);
                let ist = || Style::new().fill(fill.clone()).stroke(solid(&border, 1.0));
                scene.push_rect(ix, iy, 14.0, 10.0, 0.0, ist());
                scene.push_rect(ix - 3.0, iy + 2.0, 6.0, 2.5, 0.0, ist());
                scene.push_rect(ix - 3.0, iy + 5.5, 6.0, 2.5, 0.0, ist());
            }
            centered(scene, cx - if uml2 { 8.0 } else { 0.0 }, cy - th / 2.0);
        }
        DKind::Node => {
            box3d(scene, r, 10.0, fill.clone(), &border, bw);
            centered(scene, cx - 5.0, cy - th / 2.0 + 4.0);
        }
        DKind::Database => {
            cylinder(scene, r, 8.0, fill.clone(), &border, bw);
            centered(scene, cx, cy - th / 2.0 + 8.0);
        }
        DKind::Cloud => {
            scene.push_path(cloud_path(r), st());
            centered(scene, cx, cy - th / 2.0);
        }
        DKind::Package | DKind::Folder | DKind::Frame => {
            let look = GroupLook { fill: Some(fill.clone()), tab_fill: fill.clone(), border: border.clone(), width: bw, dashed: e.look.dashed };
            draw_group(scene, m, e.kind.group_style(), r, None, 0.0, &look);
            centered(scene, cx, cy - th / 2.0 + 6.0);
        }
        DKind::Hexagon => {
            let c = 12.0;
            scene.push_polygon(&[(r.x + c, r.y), (r.right() - c, r.y), (r.right(), cy), (r.right() - c, r.bottom()), (r.x + c, r.bottom()), (r.x, cy)], st());
            centered(scene, cx, cy - th / 2.0);
        }
        DKind::Storage => {
            scene.push_rect(r.x, r.y, r.w, r.h, r.h / 2.0, st());
            centered(scene, cx, cy - th / 2.0);
        }
        DKind::Queue => {
            scene.push_rect(r.x, r.y, r.w, r.h, r.h / 2.5, st());
            scene.push_polyline(&[(r.right() - r.h / 2.5 - 2.0, r.y + 2.0), (r.right() - r.h / 2.5 - 2.0, r.bottom() - 2.0)], solid(&border, 1.0));
            centered(scene, cx - 4.0, cy - th / 2.0);
        }
        DKind::File => {
            note_shape(scene, NoteKind::Note, r.x, r.y, r.w, r.h, fill.clone(), &border, bw);
            centered(scene, cx, cy - th / 2.0);
        }
        DKind::Artifact => {
            scene.push_rect(r.x, r.y, r.w, r.h, 0.0, st());
            let (ix, iy) = (r.right() - 20.0, r.y + 5.0);
            note_shape(scene, NoteKind::Note, ix, iy, 11.0, 14.0, Paint::solid(skin::WHITE), &border, 1.0);
            centered(scene, cx - 4.0, cy - th / 2.0);
        }
        DKind::Label => centered(scene, cx, cy - th / 2.0),
        DKind::Collections => {
            scene.push_rect(r.x + 4.0, r.y, r.w - 4.0, r.h - 4.0, 0.0, st());
            scene.push_rect(r.x, r.y + 4.0, r.w - 4.0, r.h - 4.0, 0.0, st());
            centered(scene, cx - 2.0, cy - th / 2.0 + 2.0);
        }
        _ => {
            scene.push_rect(r.x, r.y, r.w, r.h, 0.0, st());
            centered(scene, cx, cy - th / 2.0);
        }
    }
}
