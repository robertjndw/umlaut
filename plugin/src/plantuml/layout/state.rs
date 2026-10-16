//! State diagram layout. States and pseudo states are boxes for `graph`, composite
//! states are clusters (their regions are clusters inside them, stacked by the graph
//! layout), then everything is drawn in PlantUML's look: rounded boxes with a divider
//! under the name, a black dot for the start, a ringed dot for the end.

use super::gdraw::{self, draw_note, graph_opts, note_size, GraphInfo, Slot};
use super::graph::{self, Arrange, Border, Cluster, Graph, Node, Rect};
use super::shapes::{dashed, solid};
use super::Body;
use crate::plantuml::diagrams::state::*;
use crate::plantuml::model::{Measured};
use crate::plantuml::skin::{self, Skin};
use crate::scene::{HAlign, Paint, Run, Scene, Style, TextBox};

const START_FILL: &str = "#222222";

fn desc_size(s: &State, m: &Measured) -> (f32, f32) {
    let w = s.desc.iter().map(|d| m.w(*d)).fold(0.0, f32::max);
    let h = s.desc.iter().map(|d| m.h(*d) + 4.0).sum::<f32>();
    (w, h)
}

fn node_size(s: &State, m: &Measured, lr: bool) -> (f32, f32, Border) {
    match s.kind {
        SKind::Start => (20.0, 20.0, Border::Ellipse),
        SKind::End => (22.0, 22.0, Border::Ellipse),
        SKind::Choice => (26.0, 26.0, Border::Diamond),
        SKind::Fork | SKind::Join => if lr { (7.0, 80.0, Border::Rect) } else { (80.0, 7.0, Border::Rect) },
        SKind::History | SKind::DeepHistory => (24.0, 24.0, Border::Ellipse),
        SKind::EntryPoint | SKind::ExitPoint => (14.0, 14.0, Border::Ellipse),
        SKind::State => {
            let (nw, nh) = s.name.map_or((0.0, 0.0), |n| m.size(n));
            let (dw, dh) = desc_size(s, m);
            let w = (nw.max(dw) + 24.0).max(40.0);
            let h = if s.desc.is_empty() { (nh + 22.0).max(36.0) } else { nh + 14.0 + dh + 6.0 };
            (w, h, Border::Rect)
        }
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
    // Regions are created after the members they adopt, so a parent can come later in
    // the list than its children: hand out all slots first, then fill in the parents.
    let is_cluster = |i: usize| match &model.ents[i].def {
        Def::Region => true,
        Def::State(_) => has_children[i],
        Def::Note(_) => false,
    };
    let (mut nc, mut nn) = (0, 0);
    for (i, e) in model.ents.iter().enumerate() {
        if is_cluster(i) {
            slots.push(Slot::Cluster(nc));
            nc += 1;
        } else if matches!(&e.def, Def::Note(nt) if nt.on_link.is_some()) {
            slots.push(Slot::Floating);
        } else {
            slots.push(Slot::Node(nn));
            nn += 1;
        }
    }
    let parent_of = |p: Option<usize>| p.and_then(|p| slots[p].cluster());
    g.clusters = vec![Cluster { parent: None, inset: [0.0; 4], min: (0.0, 0.0), arrange: Arrange::Graph, title_centered: true }; nc];
    g.nodes = vec![Node { w: 0.0, h: 0.0, parent: None, border: Border::Rect }; nn];
    for (i, e) in model.ents.iter().enumerate() {
        let parent = parent_of(e.parent);
        match (&e.def, &slots[i]) {
            (Def::Region, Slot::Cluster(c)) => g.clusters[*c] = Cluster { parent, inset: [6.0, 6.0, 6.0, 6.0], min: (20.0, 20.0), arrange: Arrange::Graph, title_centered: true },
            (Def::State(s), Slot::Cluster(c)) => {
                let (nw, nh) = s.name.map_or((0.0, 0.0), |x| m.size(x));
                let arrange = match model.stacked.iter().find(|(c, _)| *c == i) {
                    Some((_, true)) => Arrange::StackV,
                    Some((_, false)) => Arrange::StackH,
                    None => Arrange::Graph,
                };
                let inset = if arrange == Arrange::Graph { [12.0, nh + 24.0, 12.0, 12.0] } else { [8.0, nh + 24.0, 8.0, 8.0] };
                g.clusters[*c] = Cluster { parent, inset, min: (nw + 28.0, nh + 44.0), arrange, title_centered: true };
            }
            (Def::State(s), Slot::Node(x)) => {
                let (w, h, border) = node_size(s, m, model.lr);
                g.nodes[*x] = Node { w, h, parent, border };
            }
            (Def::Note(nt), Slot::Node(x)) => {
                let (w, h) = note_size(m, nt.label);
                g.nodes[*x] = Node { w, h, parent, border: Border::Rect };
            }
            _ => {}
        }
    }
    let edge_of_link = gdraw::build_edges(&mut g, &slots, &model.links, m, false);
    let opts = graph_opts(skin, model.lr);
    let lay = graph::layout(&g, &opts)?;

    let fill_of = |look: &crate::plantuml::diagrams::gcommon::Look| look.fill.clone().unwrap_or_else(|| skin.fill(&["state"], skin::ACTIVITY_FILL));
    let border = skin.border(&["state"], skin::BORDER);
    let radius = skin.radius(&["state"], 25.0);
    let arrow_color = skin.color(&["statearrowcolor", "arrowcolor"], skin::BORDER);
    let start_color = skin.color(&["statestartcolor", "startcolor"], START_FILL);
    let mut body = Scene::new(0.0, 0.0);
    let mut titles = Vec::new();
    let mut first_region: Vec<Option<usize>> = vec![None; model.ents.len()];
    for (i, e) in model.ents.iter().enumerate() {
        if let (Def::Region, Some(p)) = (&e.def, e.parent) {
            first_region[p].get_or_insert(i);
        }
    }

    // Containers first, outermost first (parents come before their members in the list).
    for (i, e) in model.ents.iter().enumerate() {
        let Slot::Cluster(c) = slots[i] else { continue };
        let r = lay.clusters[c];
        match &e.def {
            Def::State(s) => {
                let line = s.look.line.clone().unwrap_or_else(|| border.clone());
                let st = Style::new().stroke(gdraw::outline(&line, 1.0, s.look.dashed));
                body.push_rect(r.x, r.y, r.w, r.h, radius, st.fill(fill_of(&s.look)));
                if let Some(nm) = s.name {
                    let (nw, nh) = m.size(nm);
                    let cx = r.x + r.w / 2.0 + lay.title_dx(&r, (nw, nh));
                    m.text_at(&mut body, nm, cx, r.y + 6.0, HAlign::Center);
                    titles.push((i, Rect::new(cx - nw / 2.0, r.y + 6.0, nw, nh)));
                    let ly = r.y + nh + 16.0;
                    body.push_polyline(&[(r.x, ly), (r.right(), ly)], solid(&line, 1.0));
                }
            }
            Def::Region => {
                // The first region of a stack has no divider above it.
                let comp = e.parent.and_then(|p| model.stacked.iter().find(|(c, _)| *c == p));
                let first = e.parent.is_none_or(|p| first_region[p] == Some(i));
                if let (Some((_, vertical)), false) = (comp, first) {
                    let pts = if *vertical { [(r.x, r.y), (r.right(), r.y)] } else { [(r.x, r.y), (r.x, r.bottom())] };
                    body.push_polyline(&pts, dashed(&border, 1.0, &[6.0, 4.0]));
                }
            }
            Def::Note(_) => {}
        }
    }

    let is_note = |i: usize| matches!(model.ents[i].def, Def::Note(_));
    gdraw::draw_links(&mut body, &lay, &model.links, &edge_of_link, is_note, &arrow_color, false, skin, m);

    let mut floating: Vec<Rect> = Vec::new();
    for (e, slot) in model.ents.iter().zip(&slots) {
        match (&e.def, slot) {
            (Def::State(s), Slot::Node(i)) => draw_node(&mut body, s, &lay.nodes[*i], m, &fill_of(&s.look), s.look.line.as_deref().unwrap_or(&border), radius, &start_color),
            (Def::Note(nt), Slot::Node(i)) => draw_note(&mut body, m, nt.label, &lay.nodes[*i], nt.look.fill.clone(), skin),
            (Def::Note(nt), Slot::Floating) => gdraw::draw_link_note(&mut body, &lay, &edge_of_link, nt.on_link, nt.label, nt.look.fill.clone(), &mut floating, skin, m),
            _ => {}
        }
    }

    let (mut out, _, _) = gdraw::finish(body);
    let mut info = gdraw::collect_info(&lay, &g, &slots, &edge_of_link, titles);
    let names = model
        .ents
        .iter()
        .map(|e| match &e.def {
            Def::State(s) if matches!(s.kind, SKind::Start | SKind::End) => "[*]".to_string(),
            Def::State(s) => s.name.map_or_else(|| e.key.clone(), |n| m.plain(n)),
            Def::Region => "a region".to_string(),
            Def::Note(_) if e.key.is_empty() => "a note".to_string(),
            Def::Note(_) => e.key.clone(),
        })
        .collect();
    info.report(&mut out.scene, names, model.ents.iter().map(|e| e.parent).collect(), &model.links, m);
    Ok((out, info))
}

fn draw_node(scene: &mut Scene, s: &State, r: &Rect, m: &Measured, fill: &Paint, border: &str, radius: f32, start: &str) {
    let (cx, cy) = r.center();
    match s.kind {
        SKind::Start => scene.push_ellipse(r.x, r.y, r.w, r.h, Style::new().fill(Paint::solid(start))),
        SKind::End => {
            scene.push_ellipse(r.x + 0.5, r.y + 0.5, r.w - 1.0, r.h - 1.0, Style::new().stroke(solid(start, 1.0)));
            scene.push_ellipse(cx - 6.0, cy - 6.0, 12.0, 12.0, Style::new().fill(Paint::solid(start)));
        }
        SKind::Choice => scene.push_polygon(&[(cx, r.y), (r.right(), cy), (cx, r.bottom()), (r.x, cy)], Style::new().fill(fill.clone()).stroke(solid(border, 1.0))),
        SKind::Fork | SKind::Join => scene.push_rect(r.x, r.y, r.w, r.h, 1.0, Style::new().fill(Paint::solid(start))),
        SKind::History | SKind::DeepHistory => {
            scene.push_ellipse(r.x, r.y, r.w, r.h, Style::new().fill(fill.clone()).stroke(solid(border, 1.0)));
            let text = if s.kind == SKind::History { "H" } else { "H*" };
            scene.push_text(TextBox::new(r.x, r.y, r.w, r.h, vec![Run::new(text).size(12.0).color(skin::BLACK)]).wrap(false));
        }
        SKind::EntryPoint | SKind::ExitPoint => {
            scene.push_ellipse(r.x, r.y, r.w, r.h, Style::new().fill(fill.clone()).stroke(solid(border, 1.0)));
            if s.kind == SKind::ExitPoint {
                scene.push_polyline(&[(cx - 3.5, cy - 3.5), (cx + 3.5, cy + 3.5)], solid(border, 1.0));
                scene.push_polyline(&[(cx - 3.5, cy + 3.5), (cx + 3.5, cy - 3.5)], solid(border, 1.0));
            }
            if let Some(nm) = s.name {
                m.text_at(scene, nm, cx, r.bottom() + 3.0, HAlign::Center);
            }
        }
        SKind::State => {
            scene.push_rect(r.x, r.y, r.w, r.h, radius, Style::new().fill(fill.clone()).stroke(solid(border, 1.0)));
            let Some(nm) = s.name else { return };
            let (_, nh) = m.size(nm);
            if s.desc.is_empty() {
                m.text_at(scene, nm, cx, cy - nh / 2.0, HAlign::Center);
            } else {
                m.text_at(scene, nm, cx, r.y + 7.0, HAlign::Center);
                let ly = r.y + nh + 14.0;
                scene.push_polyline(&[(r.x, ly), (r.right(), ly)], solid(border, 1.0));
                let mut y = ly + 5.0;
                for d in &s.desc {
                    m.text_at(scene, *d, r.x + 8.0, y, HAlign::Left);
                    y += m.h(*d) + 4.0;
                }
            }
        }
    }
}
