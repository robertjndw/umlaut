//! Class diagram layout: boxes sized from their labels, placed by `graph`, then drawn.
//!
//! A class box is a header (circled letter, stereotype, name), then compartments. The
//! compartments are the fields and the methods unless the body has explicit separators,
//! in which case the separators cut the body into blocks. Empty compartments are drawn
//! as thin strips like PlantUML does, unless `hide empty members` says otherwise.

use super::gdraw::{self, draw_group, draw_note, graph_opts, group_metrics, note_size, title_centered, GraphInfo, GroupLook, Slot};
use super::graph::{self, Border, Cluster, Graph, Node, Rect};
use super::shapes::{dashed, solid};
use super::Body;
use crate::plantuml::diagrams::class::*;
use crate::plantuml::model::{LabelId, Measured};
use crate::plantuml::skin::{self, Skin};
use crate::scene::{HAlign, Paint, Run, Scene, Style, TextBox};

const HEADER_PAD: f32 = 5.0;
const CIRCLE_R: f32 = 11.0;
const EMPTY_COMP: f32 = 8.0;
const COMP_PAD: f32 = 4.0;
const SIDE_PAD: f32 = 6.0;
const ICON_COL: f32 = 14.0;
/// Measured text is as tall as its glyphs; rows need air between the lines.
const ROW_PAD: f32 = 5.0;

fn kind_prefix(k: CKind) -> &'static str {
    match k {
        CKind::Class => "class",
        CKind::Abstract => "abstractclass",
        CKind::Interface => "interface",
        CKind::Enum => "enum",
        CKind::Annotation => "annotation",
        CKind::Entity => "entity",
        CKind::Protocol => "protocol",
        CKind::Struct => "struct",
        CKind::Exception => "exception",
        CKind::Metaclass => "metaclass",
        CKind::Stereotype => "stereotype",
    }
}

/// One compartment: its divider (for blocks that follow a separator) and members.
struct Comp<'a> {
    sep: Option<(char, Option<LabelId>)>,
    members: Vec<&'a Member>,
}

fn compartments<'a>(c: &'a Class) -> Vec<Comp<'a>> {
    let keep = |m: &Member| if m.method { c.show.methods } else { c.show.fields };
    if c.items.iter().any(|i| matches!(i, Item::Sep { .. })) {
        let mut out: Vec<Comp> = vec![Comp { sep: None, members: Vec::new() }];
        for it in &c.items {
            match it {
                Item::Sep { style, title } => out.push(Comp { sep: Some((*style, *title)), members: Vec::new() }),
                Item::Member(m) if keep(m) => out.last_mut().map(|x| x.members.push(m)).unwrap_or(()),
                Item::Member(_) => {}
            }
        }
        if out[0].members.is_empty() && out.len() > 1 {
            out.remove(0);
        }
        return out;
    }
    let members = |method: bool| -> Vec<&Member> { c.items.iter().filter_map(|i| if let Item::Member(m) = i { Some(m) } else { None }).filter(|m| m.method == method).collect() };
    let mut out = Vec::new();
    if c.show.fields {
        let f = members(false);
        if !f.is_empty() || c.show.empty_fields {
            out.push(Comp { sep: None, members: f });
        }
    }
    if c.show.methods {
        let m = members(true);
        if !m.is_empty() || c.show.empty_methods {
            out.push(Comp { sep: None, members: m });
        }
    }
    out
}

struct Metrics<'a> {
    w: f32,
    h: f32,
    header_h: f32,
    content_w: f32,
    comps: Vec<Comp<'a>>,
    comp_h: Vec<f32>,
    icons: bool,
}

fn metrics<'a>(c: &'a Class, m: &Measured, skin: &Skin) -> Metrics<'a> {
    let icons = skin.num(&["classattributeiconsize"], 10.0) > 0.0;
    let (nw, nh) = m.size(c.name);
    let (sw, sh) = c.stereo.filter(|_| c.show.stereo).map_or((0.0, 0.0), |s| m.size(s));
    let circle_w = if c.show.circle { 2.0 * CIRCLE_R + 4.0 } else { 0.0 };
    let text_w = nw.max(sw);
    let content_w = circle_w + text_w;
    let sgap = if sh > 0.0 { 3.0 } else { 0.0 };
    let header_h = (nh + sh + sgap).max(if c.show.circle { 2.0 * CIRCLE_R } else { 0.0 }) + 2.0 * HEADER_PAD;
    let mut w = content_w + 2.0 * 7.0;
    let comps = compartments(c);
    let mut comp_h = Vec::new();
    for comp in &comps {
        let mut h = 0.0;
        if let Some((_, title)) = comp.sep {
            h += title.map_or(0.0, |t| m.h(t));
            if let Some(t) = comp.sep.and_then(|s| s.1) {
                w = w.max(m.w(t) + 24.0);
            }
        }
        if comp.members.is_empty() {
            h += EMPTY_COMP.max(if comp.sep.is_some() { 6.0 } else { 0.0 });
        } else {
            h += 2.0 * COMP_PAD + comp.members.iter().map(|x| m.h(x.label) + ROW_PAD).sum::<f32>();
        }
        for x in &comp.members {
            let indent = if x.vis.is_some() && icons { ICON_COL } else { 0.0 };
            w = w.max(SIDE_PAD + indent + m.w(x.label) + SIDE_PAD);
        }
        comp_h.push(h);
    }
    let h = header_h + comp_h.iter().sum::<f32>();
    Metrics { w: w.max(30.0), h, header_h, content_w, comps, comp_h, icons }
}

fn icon(scene: &mut Scene, vis: char, method: bool, cx: f32, cy: f32) {
    let (fill, border) = match vis {
        '-' => ("#F24D5C", "#C82930"),
        '#' => ("#FFFF44", "#B38D22"),
        '~' => ("#4D9EFF", "#1963A0"),
        _ => ("#84BE52", "#038048"),
    };
    let st = Style::new().fill(Paint::solid(if method { fill } else { "#FFFFFF" })).stroke(solid(border, 1.0));
    match vis {
        '-' => scene.push_rect(cx - 3.5, cy - 3.5, 7.0, 7.0, 0.0, st),
        '#' => scene.push_polygon(&[(cx, cy - 5.0), (cx + 4.5, cy), (cx, cy + 5.0), (cx - 4.5, cy)], st),
        '~' => scene.push_polygon(&[(cx, cy - 4.5), (cx + 4.5, cy + 3.5), (cx - 4.5, cy + 3.5)], st),
        _ => scene.push_ellipse(cx - 4.0, cy - 4.0, 8.0, 8.0, st),
    }
}

fn separator(scene: &mut Scene, m: &Measured, style: char, title: Option<LabelId>, x: f32, y: f32, w: f32, color: &str) {
    let line = |scene: &mut Scene, x0: f32, x1: f32, y: f32| match style {
        '.' => scene.push_polyline(&[(x0, y), (x1, y)], dashed(color, 1.0, &[1.5, 2.0])),
        '=' => {
            scene.push_polyline(&[(x0, y - 1.0), (x1, y - 1.0)], solid(color, 1.0));
            scene.push_polyline(&[(x0, y + 1.0), (x1, y + 1.0)], solid(color, 1.0));
        }
        '_' => scene.push_polyline(&[(x0, y), (x1, y)], solid(color, 2.0)),
        _ => scene.push_polyline(&[(x0, y), (x1, y)], solid(color, 1.0)),
    };
    match title {
        None => line(scene, x, x + w, y),
        Some(t) => {
            let (tw, th) = m.size(t);
            let (a, b) = (x + (w - tw) / 2.0 - 4.0, x + (w + tw) / 2.0 + 4.0);
            line(scene, x, a, y + th / 2.0);
            line(scene, b, x + w, y + th / 2.0);
            m.text_at(scene, t, x + w / 2.0, y, HAlign::Center);
        }
    }
}

struct Env<'a> {
    skin: &'a Skin,
    m: &'a Measured,
}

fn draw_class(scene: &mut Scene, c: &Class, mt: &Metrics, r: &Rect, lk: &Env) {
    let (skin, m) = (lk.skin, lk.m);
    let pre = [kind_prefix(c.kind), "class"];
    let fill = c.look.fill.clone().unwrap_or_else(|| skin.fill(&pre, skin::ACTIVITY_FILL));
    let border = c.look.line.clone().unwrap_or_else(|| skin.border(&pre, skin::BORDER));
    let bw = if c.look.bold { 2.0 } else { 1.0 };
    scene.push_rect(r.x, r.y, r.w, r.h, 0.0, Style::new().fill(fill).stroke(gdraw::outline(&border, bw, c.look.dashed)));

    let (nw, nh) = m.size(c.name);
    let stereo = c.stereo.filter(|_| c.show.stereo);
    let (sw, sh) = stereo.map_or((0.0, 0.0), |s| m.size(s));
    let circle_w = if c.show.circle { 2.0 * CIRCLE_R + 4.0 } else { 0.0 };
    let text_w = nw.max(sw);
    let left = r.x + (r.w - mt.content_w) / 2.0;
    if c.show.circle {
        let (letter, spot_fill) = match &c.spot {
            Some((l, col)) => (*l, col.clone().unwrap_or_else(|| c.kind.circle_fill().to_string())),
            None => (c.kind.letter(), c.kind.circle_fill().to_string()),
        };
        let (cx, cy) = (left + CIRCLE_R, r.y + mt.header_h / 2.0);
        gdraw::spot(scene, cx, cy, CIRCLE_R, &spot_fill, &border);
        let run = Run::new(letter.to_string()).size(14.0).bold().color(skin::BLACK);
        scene.push_text(TextBox::new(cx - CIRCLE_R, cy - CIRCLE_R, 2.0 * CIRCLE_R, 2.0 * CIRCLE_R, vec![run]).wrap(false));
    }
    let tx = left + circle_w + text_w / 2.0;
    let sgap = if sh > 0.0 { 3.0 } else { 0.0 };
    let top = r.y + (mt.header_h - (nh + sh + sgap)) / 2.0;
    if let Some(s) = stereo {
        m.text_at(scene, s, tx, top, HAlign::Center);
    }
    m.text_at(scene, c.name, tx, top + sh + sgap, HAlign::Center);
    if let Some(g) = c.generics {
        let (gw, gh) = m.size(g);
        let (bw_, bh) = (gw + 10.0, gh + 4.0);
        let (bx, by) = (r.right() - bw_ + 4.0, r.y - 4.0);
        scene.push_rect(bx, by, bw_, bh, 0.0, Style::new().fill(Paint::solid(skin::WHITE)).stroke(dashed(&border, 1.0, &[2.0, 2.0])));
        m.text_at(scene, g, bx + bw_ / 2.0, by + 2.0, HAlign::Center);
    }

    let mut y = r.y + mt.header_h;
    for (comp, &h) in mt.comps.iter().zip(&mt.comp_h) {
        match comp.sep {
            Some((style, title)) => separator(scene, m, style, title, r.x, y, r.w, &border),
            None => scene.push_polyline(&[(r.x, y), (r.right(), y)], solid(&border, 1.0)),
        }
        let mut ry = y + COMP_PAD + comp.sep.and_then(|s| s.1).map_or(0.0, |t| m.h(t));
        for x in &comp.members {
            let rh = m.h(x.label) + ROW_PAD;
            let mut tx = r.x + SIDE_PAD;
            if let (Some(v), true) = (x.vis, mt.icons) {
                icon(scene, v, x.method, r.x + SIDE_PAD + 5.0, ry + rh / 2.0);
                tx += ICON_COL;
            }
            m.text_at(scene, x.label, tx, ry + ROW_PAD / 2.0, HAlign::Left);
            ry += rh;
        }
        y += h;
    }
}

pub fn layout(model: &Model, skin: &Skin, m: &Measured) -> Result<Body, String> {
    layout_info(model, skin, m).map(|r| r.0)
}

pub fn layout_info(model: &Model, skin: &Skin, m: &Measured) -> Result<(Body, GraphInfo), String> {
    let mut g = Graph::default();
    let mut slots: Vec<Slot> = Vec::new();
    // Clusters first so members can name their cluster index.
    for e in &model.ents {
        let parent = e.parent.and_then(|p| slots[p].cluster());
        match &e.def {
            Def::Group(grp) => {
                let (inset, min) = group_metrics(grp.style, grp.label.map(|t| m.size(t)));
                g.clusters.push(Cluster { parent, inset, min, arrange: graph::Arrange::Graph, title_centered: title_centered(grp.style) });
                slots.push(Slot::Cluster(g.clusters.len() - 1));
            }
            Def::Class(c) => {
                let mt = metrics(c, m, skin);
                g.nodes.push(Node { w: mt.w, h: mt.h, parent, border: Border::Rect });
                slots.push(Slot::Node(g.nodes.len() - 1));
            }
            Def::Note(n) => {
                if n.on_link.is_some() {
                    slots.push(Slot::Floating);
                    continue;
                }
                let (w, h) = note_size(m, n.label);
                g.nodes.push(Node { w, h, parent, border: Border::Rect });
                slots.push(Slot::Node(g.nodes.len() - 1));
            }
        }
    }
    let edge_of_link = gdraw::build_edges(&mut g, &slots, &model.links, m, true);
    let opts = graph_opts(skin, model.dir);
    let lay = graph::layout(&g, &opts)?;

    let mut body = Scene::new(0.0, 0.0);
    let lk = Env { skin, m };
    let border = skin.border(&["package"], skin::BORDER);
    let arrow_color = skin.color(&["classarrowcolor", "arrowcolor"], skin::BORDER);

    let mut titles = Vec::new();
    for (i, (e, slot)) in model.ents.iter().zip(&slots).enumerate() {
        if let (Def::Group(grp), Slot::Cluster(c)) = (&e.def, slot) {
            let look = GroupLook {
                fill: grp.look.fill.clone(),
                tab_fill: skin.fill(&["package"], skin::GROUP_TAB_FILL),
                border: grp.look.line.clone().unwrap_or_else(|| border.clone()),
                width: 1.0,
                dashed: grp.look.dashed,
            };
            let dx = grp.label.map_or(0.0, |t| lay.title_dx(&lay.clusters[*c], m.size(t)));
            if let Some(t) = draw_group(&mut body, m, grp.style, &lay.clusters[*c], grp.label, dx, &look) {
                titles.push((i, t));
            }
        }
    }
    let is_note = |i: usize| matches!(model.ents[i].def, Def::Note(_));
    gdraw::draw_links(&mut body, &lay, &model.links, &edge_of_link, is_note, &arrow_color, true, skin, m);
    let mut floating: Vec<Rect> = Vec::new();
    for (e, slot) in model.ents.iter().zip(&slots) {
        match (&e.def, slot) {
            (Def::Class(c), Slot::Node(n)) => {
                let mt = metrics(c, m, skin);
                draw_class(&mut body, c, &mt, &lay.nodes[*n], &lk);
            }
            (Def::Note(n), Slot::Node(i)) => draw_note(&mut body, m, n.label, &lay.nodes[*i], n.look.fill.clone(), skin),
            (Def::Note(n), Slot::Floating) => gdraw::draw_link_note(&mut body, &lay, &edge_of_link, n.on_link, n.label, n.look.fill.clone(), &mut floating, skin, m),
            _ => {}
        }
    }

    let (mut out, _, _) = gdraw::finish(body);
    let mut info = gdraw::collect_info(&lay, &g, &slots, &edge_of_link, titles);
    let names = model.ents.iter().map(|e| if e.key.is_empty() && matches!(e.def, Def::Note(_)) { "a note".to_string() } else { e.key.clone() }).collect();
    info.report(&mut out.scene, names, model.ents.iter().map(|e| e.parent).collect(), &model.links, m);
    Ok((out, info))
}
