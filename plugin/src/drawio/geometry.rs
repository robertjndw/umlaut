//! The cell tree of an `<mxGraphModel>`: parsing, absolute coordinates for cells nested
//! in containers, and the final translation of the scene to the origin.

use std::collections::HashMap;

use roxmltree::{Document, Node};

use crate::scene::{Item, Scene};

use super::style::Style;

pub type Pt = (f32, f32);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn cx(&self) -> f32 {
        self.x + self.w / 2.0
    }

    pub fn cy(&self) -> f32 {
        self.y + self.h / 2.0
    }

    pub fn center(&self) -> Pt {
        (self.cx(), self.cy())
    }

    pub fn grow(&self, d: f32) -> Rect {
        Rect::new(self.x - d, self.y - d, self.w + 2.0 * d, self.h + 2.0 * d)
    }

    pub fn contains(&self, p: Pt) -> bool {
        p.0 >= self.x && p.0 <= self.right() && p.1 >= self.y && p.1 <= self.bottom()
    }

    pub fn union(&self, o: &Rect) -> Rect {
        let x = self.x.min(o.x);
        let y = self.y.min(o.y);
        Rect::new(x, y, self.right().max(o.right()) - x, self.bottom().max(o.bottom()) - y)
    }
}

/// Rotates `p` clockwise (on screen, y down) by `deg` around `c`.
pub fn rotate_about(p: Pt, c: Pt, deg: f32) -> Pt {
    if deg == 0.0 {
        return p;
    }
    let (s, co) = deg.to_radians().sin_cos();
    let (dx, dy) = (p.0 - c.0, p.1 - c.1);
    (c.0 + dx * co - dy * s, c.1 + dx * s + dy * co)
}

#[derive(Debug, Clone, Default)]
pub struct Geo {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub relative: bool,
    pub points: Vec<Pt>,
    pub source_point: Option<Pt>,
    pub target_point: Option<Pt>,
    pub offset: Option<Pt>,
}

#[derive(Debug, Clone)]
pub struct Cell {
    pub parent: Option<usize>,
    pub value: String,
    pub style: Style,
    pub vertex: bool,
    pub edge: bool,
    pub source: Option<String>,
    pub target: Option<String>,
    pub visible: bool,
    pub collapsed: bool,
    pub geo: Option<Geo>,
    pub children: Vec<usize>,
}

#[derive(Debug, Default)]
pub struct Model {
    pub cells: Vec<Cell>,
    by_id: HashMap<String, usize>,
    pub roots: Vec<usize>,
    /// Background color of the page, when the file sets one.
    pub background: Option<String>,
}

fn num(node: &Node, key: &str) -> Option<f32> {
    node.attribute(key).and_then(|v| v.trim().parse::<f32>().ok()).filter(|v| v.is_finite())
}

fn point(node: &Node) -> Pt {
    (num(node, "x").unwrap_or(0.0), num(node, "y").unwrap_or(0.0))
}

fn parse_geometry(node: Node) -> Geo {
    let mut g = Geo {
        x: num(&node, "x").unwrap_or(0.0),
        y: num(&node, "y").unwrap_or(0.0),
        // Shape code clamps parameters to `0..=size`, which panics for a negative size.
        w: num(&node, "width").unwrap_or(0.0).max(0.0),
        h: num(&node, "height").unwrap_or(0.0).max(0.0),
        relative: node.attribute("relative") == Some("1"),
        ..Default::default()
    };
    for child in node.children().filter(|c| c.is_element()) {
        match (child.tag_name().name(), child.attribute("as")) {
            ("mxPoint", Some("sourcePoint")) => g.source_point = Some(point(&child)),
            ("mxPoint", Some("targetPoint")) => g.target_point = Some(point(&child)),
            ("mxPoint", Some("offset")) => g.offset = Some(point(&child)),
            ("Array", Some("points")) => {
                g.points = child.children().filter(|p| p.has_tag_name("mxPoint")).map(|p| point(&p)).collect();
            }
            _ => {}
        }
    }
    g
}

/// Replaces `%name%` with the attribute of the wrapping `<object>`; unknown names stay.
fn substitute(value: &str, wrapper: &Node) -> String {
    if !value.contains('%') {
        return value.to_string();
    }
    let mut out = String::new();
    let mut rest = value;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) if end > 0 && !after[..end].contains(char::is_whitespace) => {
                match wrapper.attribute(&after[..end]) {
                    Some(v) => out.push_str(v),
                    None => out.push_str(&rest[start..start + end + 2]),
                }
                rest = &after[end + 1..];
            }
            _ => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

impl Model {
    pub fn parse(xml: &str) -> Result<Model, String> {
        let opts = roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() };
        super::decode::check_xml_depth(xml)?;
        let doc = Document::parse_with_options(xml, opts).map_err(|e| format!("the diagram is not valid XML: {e}"))?;
        let model_node = doc.root_element();
        if model_node.tag_name().name() != "mxGraphModel" {
            return Err(format!("expected <mxGraphModel>, found <{}>", model_node.tag_name().name()));
        }
        let root = model_node
            .children()
            .find(|n| n.has_tag_name("root"))
            .ok_or_else(|| "the diagram has no <root> element".to_string())?;

        let mut model =
            Model { background: model_node.attribute("background").map(str::to_string), ..Default::default() };
        let mut parents: Vec<Option<String>> = Vec::new();
        for node in root.children().filter(|n| n.is_element()) {
            let name = node.tag_name().name();
            // `<object>` and `<UserObject>` carry the id and label and wrap the real cell.
            let (cell_node, wrapper) = match name {
                "mxCell" => (node, None),
                "object" | "UserObject" | "Object" | "XmlLabel" => {
                    match node.children().find(|c| c.has_tag_name("mxCell")) {
                        Some(inner) => (inner, Some(node)),
                        None => continue,
                    }
                }
                _ => continue,
            };
            let id = wrapper.unwrap_or(cell_node).attribute("id").unwrap_or("").to_string();
            let raw_value = match wrapper {
                Some(w) => {
                    w.attribute("label").or_else(|| w.attribute("value")).or_else(|| cell_node.attribute("value"))
                }
                None => cell_node.attribute("value"),
            }
            .unwrap_or("");
            let value = match wrapper {
                // draw.io only expands `%name%` when the object opts in.
                Some(w) if w.attribute("placeholders") == Some("1") => substitute(raw_value, &w),
                _ => raw_value.to_string(),
            };
            let geo = cell_node
                .children()
                .find(|c| c.has_tag_name("mxGeometry") && c.attribute("as").unwrap_or("geometry") == "geometry")
                .map(parse_geometry);
            let idx = model.cells.len();
            model.by_id.insert(id.clone(), idx);
            parents.push(cell_node.attribute("parent").map(str::to_string));
            model.cells.push(Cell {
                parent: None,
                value,
                style: Style::parse(cell_node.attribute("style").unwrap_or("")),
                vertex: cell_node.attribute("vertex") == Some("1"),
                edge: cell_node.attribute("edge") == Some("1"),
                source: cell_node.attribute("source").map(str::to_string),
                target: cell_node.attribute("target").map(str::to_string),
                visible: cell_node.attribute("visible") != Some("0"),
                collapsed: cell_node.attribute("collapsed") == Some("1"),
                geo,
                children: Vec::new(),
            });
        }
        for (idx, parent) in parents.into_iter().enumerate() {
            let parent_idx = parent.and_then(|p| model.by_id.get(&p).copied()).filter(|&p| p != idx);
            model.cells[idx].parent = parent_idx;
            match parent_idx {
                Some(p) => model.cells[p].children.push(idx),
                None => model.roots.push(idx),
            }
        }
        model.check_depth()?;
        Ok(model)
    }

    /// Every pass over the cells recurses along parent links, so a long chain (or a
    /// parent cycle) has to be refused up front.
    fn check_depth(&self) -> Result<(), String> {
        for start in 0..self.cells.len() {
            let mut cur = self.cells[start].parent;
            let mut depth = 0;
            while let Some(p) = cur {
                depth += 1;
                if depth > super::decode::MAX_XML_DEPTH {
                    return Err(format!(
                        "cells are nested too deeply (more than {} levels, or a cell is its own ancestor)",
                        super::decode::MAX_XML_DEPTH
                    ));
                }
                cur = self.cells[p].parent;
            }
        }
        Ok(())
    }

    pub fn find(&self, id: &str) -> Option<usize> {
        self.by_id.get(id).copied()
    }

    /// Absolute origin of the coordinate system the children of `idx` live in.
    pub fn child_origin(&self, idx: usize) -> Pt {
        self.child_origin_depth(idx, 0)
    }

    fn child_origin_depth(&self, idx: usize, depth: u32) -> Pt {
        if depth > 1000 {
            return (0.0, 0.0);
        }
        let c = &self.cells[idx];
        if c.vertex {
            if let Some(r) = self.vertex_rect(idx) {
                return (r.x, r.y);
            }
        }
        match c.parent {
            Some(p) => self.child_origin_depth(p, depth + 1),
            None => (0.0, 0.0),
        }
    }

    /// Absolute bounds of a vertex. `None` for cells without geometry.
    pub fn vertex_rect(&self, idx: usize) -> Option<Rect> {
        self.vertex_rect_depth(idx, 0)
    }

    fn vertex_rect_depth(&self, idx: usize, depth: u32) -> Option<Rect> {
        let c = &self.cells[idx];
        let g = c.geo.as_ref()?;
        if depth > 1000 {
            return None;
        }
        let parent = c.parent;
        let parent_cell = parent.map(|p| &self.cells[p]);
        let (ox, oy) = match (parent, parent_cell) {
            (Some(p), Some(pc)) if pc.vertex => {
                let pr = self.vertex_rect_depth(p, depth + 1)?;
                if g.relative {
                    // Relative geometry on a vertex (connection ports) is a fraction of
                    // the parent's size.
                    let off = g.offset.unwrap_or((0.0, 0.0));
                    return Some(Rect::new(pr.x + g.x * pr.w + off.0, pr.y + g.y * pr.h + off.1, g.w, g.h));
                }
                (pr.x, pr.y)
            }
            (Some(p), _) => self.child_origin_depth(p, depth + 1),
            _ => (0.0, 0.0),
        };
        Some(Rect::new(ox + g.x, oy + g.y, g.w, g.h))
    }
}

/// Bounds of everything drawn, including stroke width and rotation. `None` for an
/// empty scene.
pub fn content_bounds(scene: &Scene) -> Option<Rect> {
    let mut acc: Option<Rect> = None;
    let mut add = |r: Rect| acc = Some(acc.map_or(r, |a| a.union(&r)));
    for item in &scene.items {
        match item {
            Item::Text(t) => {
                let c = (t.x + t.w / 2.0, t.y + t.h / 2.0);
                let corners = [(t.x, t.y), (t.x + t.w, t.y), (t.x + t.w, t.y + t.h), (t.x, t.y + t.h)];
                add(bounds_of(corners.iter().map(|&p| rotate_about(p, c, t.rotation))));
            }
            Item::Shape(s) => {
                let half = s.stroke.as_ref().map_or(0.0, |st| st.width / 2.0);
                let [x, y, w, h] = s.bbox;
                let c = (x + w / 2.0, y + h / 2.0);
                let corners = [(x, y), (x + w, y), (x + w, y + h), (x, y + h)];
                let r = bounds_of(corners.iter().map(|&p| rotate_about(p, c, s.rotation)));
                add(r.grow(half));
            }
        }
    }
    acc
}

pub(super) fn bounds_of(points: impl Iterator<Item = Pt>) -> Rect {
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for (x, y) in points {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(cells: &str) -> Model {
        Model::parse(&format!(
            "<mxGraphModel><root><mxCell id=\"0\"/><mxCell id=\"1\" parent=\"0\"/>{cells}</root></mxGraphModel>"
        ))
        .unwrap()
    }

    #[test]
    fn nested_cells_get_absolute_coordinates() {
        let m = model(
            r#"<mxCell id="g" vertex="1" parent="1" style="swimlane"><mxGeometry x="100" y="50" width="300" height="200" as="geometry"/></mxCell>
               <mxCell id="in" vertex="1" parent="g"><mxGeometry x="10" y="40" width="80" height="30" as="geometry"/></mxCell>
               <mxCell id="deep" vertex="1" parent="in"><mxGeometry x="5" y="5" width="10" height="10" as="geometry"/></mxCell>"#,
        );
        let r = m.vertex_rect(m.find("in").unwrap()).unwrap();
        assert_eq!((r.x, r.y, r.w, r.h), (110.0, 90.0, 80.0, 30.0));
        let r = m.vertex_rect(m.find("deep").unwrap()).unwrap();
        assert_eq!((r.x, r.y), (115.0, 95.0));
    }

    #[test]
    fn objects_wrap_cells_and_substitute_placeholders() {
        let m = model(
            r#"<object label="%name% (v%ver%) %unknown%" placeholders="1" name="API" ver="2" id="o"><mxCell vertex="1" parent="1" style="rounded=1"><mxGeometry width="10" height="10" as="geometry"/></mxCell></object>"#,
        );
        let c = &m.cells[m.find("o").unwrap()];
        assert_eq!(c.value, "API (v2) %unknown%");
        assert!(c.vertex);
    }

    #[test]
    fn negative_sizes_become_zero() {
        let m = model(
            r#"<mxCell id="n" vertex="1" parent="1" style="shape=cube"><mxGeometry width="-180" height="-5" as="geometry"/></mxCell>"#,
        );
        let g = m.cells[m.find("n").unwrap()].geo.as_ref().unwrap();
        assert_eq!((g.w, g.h), (0.0, 0.0));
    }

    #[test]
    fn placeholders_need_the_opt_in() {
        let m = model(
            r#"<object label="%name%" name="API" id="o"><mxCell vertex="1" parent="1"><mxGeometry width="10" height="10" as="geometry"/></mxCell></object>"#,
        );
        assert_eq!(m.cells[m.find("o").unwrap()].value, "%name%");
    }

    #[test]
    fn deep_parent_chains_are_refused() {
        let mut cells = String::from(r#"<mxCell id="0"/>"#);
        for i in 1..=400 {
            cells.push_str(&format!(r#"<mxCell id="{i}" vertex="1" parent="{}"><mxGeometry width="1" height="1" as="geometry"/></mxCell>"#, i - 1));
        }
        let xml = format!("<mxGraphModel><root>{cells}</root></mxGraphModel>");
        assert!(Model::parse(&xml).unwrap_err().contains("nested too deeply"));
    }

    #[test]
    fn edge_geometry_points() {
        let m = model(
            r#"<mxCell id="e" edge="1" parent="1"><mxGeometry relative="1" as="geometry"><mxPoint x="1" y="2" as="sourcePoint"/><mxPoint x="3" y="4" as="targetPoint"/><Array as="points"><mxPoint x="5" y="6"/><mxPoint x="7" y="8"/></Array></mxGeometry></mxCell>"#,
        );
        let g = m.cells[m.find("e").unwrap()].geo.as_ref().unwrap();
        assert_eq!(g.source_point, Some((1.0, 2.0)));
        assert_eq!(g.target_point, Some((3.0, 4.0)));
        assert_eq!(g.points, vec![(5.0, 6.0), (7.0, 8.0)]);
    }

    #[test]
    fn rotation_is_clockwise_on_screen() {
        let p = rotate_about((1.0, 0.0), (0.0, 0.0), 90.0);
        assert!((p.0).abs() < 1e-5 && (p.1 - 1.0).abs() < 1e-5);
    }
}
