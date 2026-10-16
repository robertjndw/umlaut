//! Wraps a laid-out diagram with the page-level texts and the background.

use super::shapes::solid;
use super::Body;
use crate::plantuml::model::{Measured, Meta};
use crate::plantuml::skin::{self, Skin};
use crate::scene::{HAlign, Item, Scene, Shape, Style};

/// Space around everything, as PlantUML leaves around its images.
pub const MARGIN: f32 = 6.0;

pub fn compose(body: Body, meta: &Meta, skin: &Skin, m: &Measured) -> Scene {
    let Body { scene: body_scene, w: bw, h: bh } = body;

    let legend_w = meta.legend.map(|(id, _)| m.w(id) + 20.0).unwrap_or(0.0);
    let widths = [
        bw,
        meta.title.map_or(0.0, |id| m.w(id)),
        meta.header.map_or(0.0, |(id, _)| m.w(id)),
        meta.footer.map_or(0.0, |(id, _)| m.w(id)),
        meta.caption.map_or(0.0, |id| m.w(id)),
        legend_w,
    ];
    let total_w = widths.iter().cloned().fold(0.0, f32::max) + 2.0 * MARGIN;

    let mut out = Scene::new(total_w, 0.0);
    out.warnings = body_scene.warnings.clone();
    let mut y = MARGIN;

    let aligned = |a: HAlign, w: f32| match a {
        HAlign::Left => MARGIN,
        HAlign::Center => (total_w - w) / 2.0,
        HAlign::Right => total_w - MARGIN - w,
    };
    let legend = |out: &mut Scene, y: &mut f32| {
        let Some((id, place)) = meta.legend else { return };
        let (tw, th) = m.size(id);
        let (w, h) = (tw + 20.0, th + 12.0);
        let x = aligned(place.h, w);
        let fill = skin.fill(&["legend"], skin::NOTE_FILL);
        let border = skin.border(&["legend"], skin::BORDER);
        out.push_rect(x, *y, w, h, 0.0, Style::new().fill(fill).stroke(solid(&border, 1.0)));
        m.text_at(out, id, x + 10.0, *y + 6.0, HAlign::Left);
        *y += h + 8.0;
    };

    if let Some((id, a)) = meta.header {
        let (w, h) = m.size(id);
        m.text_at(&mut out, id, aligned(a, w), y, HAlign::Left);
        y += h + 4.0;
    }
    if let Some(id) = meta.title {
        let (_, h) = m.size(id);
        m.text_at(&mut out, id, total_w / 2.0, y + 2.0, HAlign::Center);
        y += h + 12.0;
    }
    if matches!(meta.legend, Some((_, p)) if p.top) {
        legend(&mut out, &mut y);
    }

    let first_body_item = out.items.len();
    let bx = (total_w - bw) / 2.0;
    out.items.extend(body_scene.items);
    crate::scene::translate_items(&mut out.items[first_body_item..], bx, y);
    y += bh;

    if matches!(meta.legend, Some((_, p)) if !p.top) {
        y += 8.0;
        legend(&mut out, &mut y);
        y -= 8.0;
    }
    if let Some(id) = meta.caption {
        let (_, h) = m.size(id);
        y += 8.0;
        m.text_at(&mut out, id, total_w / 2.0, y, HAlign::Center);
        y += h;
    }
    if let Some((id, a)) = meta.footer {
        let (w, h) = m.size(id);
        y += 6.0;
        m.text_at(&mut out, id, aligned(a, w), y, HAlign::Left);
        y += h;
    }
    y += MARGIN;
    out.size[1] = y;

    if let Some(bg) = skin.background() {
        out.items.insert(0, Item::Shape(Shape::rect(0.0, 0.0, total_w, y, 0.0, Style::new().fill(bg))));
    }
    out
}
