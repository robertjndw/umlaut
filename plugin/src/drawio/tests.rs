//! Whole-pipeline tests on the fixtures in plugin/tests/fixtures/drawio.

use super::render;
use crate::scene::{plain_text, Geom, Item, PathCmd, Scene, TextBox};
use crate::{DrawioOptions, Page};

macro_rules! fixture {
    ($name:literal) => {
        include_str!(concat!("../../tests/fixtures/drawio/", $name))
    };
}

fn draw(source: &str, page: Page) -> Result<Scene, String> {
    render(source.as_bytes(), &DrawioOptions { page })
}

fn scene(source: &str) -> Scene {
    draw(source, Page::Index(0)).unwrap_or_else(|e| panic!("render failed: {e}"))
}

fn texts(s: &Scene) -> Vec<&TextBox> {
    s.items
        .iter()
        .filter_map(|i| match i {
            Item::Text(t) => Some(t),
            _ => None,
        })
        .collect()
}

fn plain(t: &TextBox) -> String {
    plain_text(&t.runs)
}

fn has_text(s: &Scene, text: &str) -> bool {
    texts(s).iter().any(|t| plain(t) == text)
}

fn all_numbers(s: &Scene) -> Vec<f32> {
    let mut v = s.size.to_vec();
    for item in &s.items {
        match item {
            Item::Text(t) => v.extend([t.x, t.y, t.w, t.h, t.rotation]),
            Item::Shape(sh) => {
                v.extend(sh.bbox);
                v.push(sh.opacity);
                match &sh.geom {
                    Geom::Rect { x, y, w, h, radius } => v.extend([*x, *y, *w, *h, *radius]),
                    Geom::Ellipse { x, y, w, h } => v.extend([*x, *y, *w, *h]),
                    Geom::Polygon { points } => v.extend(points.iter().flatten()),
                    Geom::Path { cmds } => {
                        for c in cmds {
                            match *c {
                                PathCmd::Move { x, y } | PathCmd::Line { x, y } => v.extend([x, y]),
                                PathCmd::Cubic { x1, y1, x2, y2, x, y } => v.extend([x1, y1, x2, y2, x, y]),
                                PathCmd::Close => {}
                            }
                        }
                    }
                }
            }
        }
    }
    v
}

const ALL: &[(&str, &str)] = &[
    ("flowchart", fixture!("flowchart.drawio")),
    ("edge-styles", fixture!("edge-styles.drawio")),
    ("orthogonal", fixture!("orthogonal.drawio")),
    ("markers", fixture!("markers.drawio")),
    ("containers", fixture!("containers.drawio")),
    ("html-labels", fixture!("html-labels.drawio")),
    ("shapes", fixture!("shapes.drawio")),
    ("er", fixture!("er.drawio")),
    ("compressed", fixture!("compressed.drawio")),
    ("bare", fixture!("bare.drawio")),
    ("svg", fixture!("diagram.drawio.svg")),
    ("multipage", fixture!("multipage.drawio")),
];

#[test]
fn every_fixture_renders_into_its_own_bounds() {
    for (name, source) in ALL {
        let s = scene(source);
        assert!(!s.items.is_empty(), "{name} is empty");
        assert!(s.size[0] > 8.0 && s.size[1] > 8.0, "{name}: {:?}", s.size);
        let numbers = all_numbers(&s);
        assert!(numbers.iter().all(|n| n.is_finite()), "{name} has non-finite numbers");
        // Nothing drawn left of or above the origin, and the margin is kept.
        let b = super::geometry::content_bounds(&s).unwrap();
        assert!((b.x - super::MARGIN).abs() < 0.01 && (b.y - super::MARGIN).abs() < 0.01, "{name}: {b:?}");
        assert!(
            (b.right() + super::MARGIN - s.size[0]).abs() < 0.01
                && (b.bottom() + super::MARGIN - s.size[1]).abs() < 0.01,
            "{name}"
        );
        // And it survives the trip to Typst.
        Scene::from_cbor(&s.to_cbor().unwrap()).unwrap();
    }
}

#[test]
fn flowchart_has_shapes_edges_and_labels() {
    let s = scene(fixture!("flowchart.drawio"));
    assert!(s.warnings.is_empty(), "{:?}", s.warnings);
    for label in ["Start", "Enter credentials", "Valid?", "yes", "no", "retry", "Done"] {
        assert!(has_text(&s, label), "missing {label}");
    }
    // The multi-line label keeps its break and italic run.
    let error = texts(&s).into_iter().find(|t| plain(t).starts_with("Show error")).unwrap();
    assert!(error.runs.iter().any(|r| r.br));
    assert!(error.runs.iter().any(|r| r.italic && r.text == "(max 3 tries)"));
    // Edge labels sit on a white background by default, vertex labels on none.
    let yes = texts(&s).into_iter().find(|t| plain(t) == "yes").unwrap();
    assert_eq!(yes.background.as_ref().unwrap().color, "#ffffffff");
    let start = texts(&s).into_iter().find(|t| plain(t) == "Start").unwrap();
    assert!(start.background.is_none());
}

#[test]
fn containers_use_absolute_coordinates() {
    let s = scene(fixture!("containers.drawio"));
    let rects: Vec<(f32, f32, f32, f32)> = s
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Shape(sh) => match sh.geom {
                Geom::Rect { x, y, w, h, radius } if radius > 0.0 && w == 110.0 && h == 60.0 => Some((x, y, w, h)),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(rects.len(), 4, "{rects:?}");
    // Place order (110,90) and Pay (300,90) in the first lane: same row, 190 apart.
    let (place, pay) = (rects[0], rects[1]);
    assert_eq!(place.1, pay.1);
    assert_eq!(pay.0 - place.0, 190.0);
    // Pick items is one lane (160) lower than Pay.
    assert_eq!(rects[2].1 - pay.1, 160.0);
    assert_eq!(rects[2].0, pay.0);
}

#[test]
fn hidden_and_collapsed_cells_are_skipped() {
    let s = scene(fixture!("containers.drawio"));
    assert!(!has_text(&s, "I am hidden"));
    assert!(!has_text(&s, "never shown"));
    assert!(has_text(&s, "Collapsed"));
}

#[test]
fn multi_page_selection() {
    let src = fixture!("multipage.drawio");
    let first = draw(src, Page::Index(0)).unwrap();
    assert!(has_text(&first, "Page one"));
    let by_name = draw(src, Page::Name("Details".into())).unwrap();
    assert!(has_text(&by_name, "Second page (compressed)"));
    assert!(has_text(&by_name, "Gr\u{fc}\u{df}e & 100% \u{20ac}"));
    let unnamed = draw(src, Page::Index(2)).unwrap();
    assert!(has_text(&unnamed, "Third page"));
    assert!(draw(src, Page::Name("Page-3".into())).is_ok());
}

#[test]
fn page_errors_name_the_available_pages() {
    let src = fixture!("multipage.drawio");
    let e = draw(src, Page::Name("Missing".into())).unwrap_err();
    assert!(e.contains("\"Missing\"") && e.contains("\"Overview\"") && e.contains("\"Details\""), "{e}");
    let e = draw(src, Page::Index(7)).unwrap_err();
    assert!(e.contains("7") && e.contains("3 pages"), "{e}");
}

#[test]
fn svg_and_bare_model_match_the_plain_file() {
    let svg = scene(fixture!("diagram.drawio.svg"));
    let bare = scene(fixture!("bare.drawio"));
    assert_eq!(svg, bare);
}

#[test]
fn compressed_and_plain_pages_render_the_same() {
    let compressed = draw(fixture!("multipage.drawio"), Page::Name("Details".into())).unwrap();
    let alone = scene(fixture!("compressed.drawio"));
    assert_eq!(compressed, alone);
}

#[test]
fn unsupported_things_warn_instead_of_failing() {
    let s = scene(fixture!("shapes.drawio"));
    let joined = s.warnings.join("\n");
    assert!(joined.contains("mxgraph.aws4.lambda"), "{joined}");
    assert!(joined.contains("image"), "{joined}");
    assert!(joined.contains("sketch"), "{joined}");
    // Shadows are cosmetic and stay quiet.
    assert!(!joined.contains("shadow"), "{joined}");
    let s = scene(fixture!("edge-styles.drawio"));
    assert!(s.warnings.iter().any(|w| w.contains("isometric")), "{:?}", s.warnings);
}

#[test]
fn html_labels_become_runs() {
    let s = scene(fixture!("html-labels.drawio"));
    let rich = texts(&s).into_iter().find(|t| plain(t).starts_with("Bold, italic")).unwrap();
    assert!(rich.runs.iter().any(|r| r.bold && r.text == "Bold"));
    assert!(rich.runs.iter().any(|r| r.italic && r.text == "italic"));
    assert!(rich.runs.iter().any(|r| r.underline && r.text == "underline"));
    assert!(rich.runs.iter().any(|r| r.strike && r.text == "strike"));
    let mixed = texts(&s).into_iter().find(|t| plain(t).contains("Red 18px")).unwrap();
    assert!(mixed.runs.iter().any(|r| r.color == "#cc0000ff" && r.size == 18.0));
    assert!(mixed.runs.iter().any(|r| r.font.as_deref() == Some("Courier New")));
    assert!(plain(mixed).contains("& <escaped>"));
    // Rotated, right-aligned, outside labels.
    let rotated = texts(&s).into_iter().find(|t| plain(t) == "Rotated text").unwrap();
    assert_eq!(rotated.rotation, -90.0);
    // The label hangs below the stick figure instead of sitting on it.
    let stick = texts(&s).into_iter().find(|t| plain(t) == "Stickman").unwrap();
    let figure_top = s
        .items
        .iter()
        .filter_map(|i| if let Item::Shape(sh) = i { Some(sh.bbox) } else { None })
        .filter(|b| b[0] <= stick.x + stick.w / 2.0 && b[0] + b[2] >= stick.x + stick.w / 2.0 && b[1] < stick.y)
        .map(|b| b[1])
        .fold(f32::MAX, f32::min);
    assert!(stick.y - figure_top >= 60.0, "label at {} figure at {figure_top}", stick.y);
}

#[test]
fn er_markers_and_table_rows_render() {
    let s = scene(fixture!("er.drawio"));
    for label in ["Customer", "Order", "customer_id", "places", "contains", "PK", "FK"] {
        assert!(has_text(&s, label), "missing {label}");
    }
    assert!(s.warnings.is_empty(), "{:?}", s.warnings);
}

#[test]
fn degenerate_input_does_not_panic() {
    let wrap = |cells: &str| {
        format!("<mxGraphModel><root><mxCell id=\"0\"/><mxCell id=\"1\" parent=\"0\"/>{cells}</root></mxGraphModel>")
    };
    let cases = [
        // No geometry, zero size, NaN and huge numbers, missing terminals, cycles.
        r#"<mxCell id="a" value="x" style="rounded=1" vertex="1" parent="1"/>"#,
        r#"<mxCell id="a" value="x" vertex="1" parent="1"><mxGeometry width="0" height="0" as="geometry"/></mxCell>"#,
        r#"<mxCell id="a" value="x" vertex="1" parent="1"><mxGeometry x="NaN" y="inf" width="1e30" height="-5" as="geometry"/></mxCell>"#,
        r#"<mxCell id="e" edge="1" parent="1" source="nope" target="nada"><mxGeometry relative="1" as="geometry"/></mxCell>"#,
        r#"<mxCell id="e" edge="1" parent="1" style="edgeStyle=orthogonalEdgeStyle"><mxGeometry relative="1" as="geometry"/></mxCell>"#,
        r#"<mxCell id="p" vertex="1" parent="q"><mxGeometry width="1" height="1" as="geometry"/></mxCell><mxCell id="q" vertex="1" parent="p"><mxGeometry width="1" height="1" as="geometry"/></mxCell>"#,
        r#"<mxCell id="a" vertex="1" parent="1" style="shape=table;startSize=999;horizontal=0"><mxGeometry width="10" height="10" as="geometry"/></mxCell>"#,
        r#"<mxCell id="a" value="&lt;div&gt;&lt;b&gt;&lt;i" style="html=1" vertex="1" parent="1"><mxGeometry width="50" height="20" as="geometry"/></mxCell>"#,
        r#"<mxCell id="a" vertex="1" parent="1"><mxGeometry width="50" height="50" as="geometry"/></mxCell><mxCell id="e" edge="1" parent="1" source="a" target="a" style="edgeStyle=elbowEdgeStyle;exitX=2;entryY=-3"><mxGeometry relative="1" as="geometry"/></mxCell>"#,
    ];
    for case in cases {
        if let Ok(s) = draw(&wrap(case), Page::Index(0)) {
            assert!(all_numbers(&s).iter().all(|n| n.is_finite()), "{case}");
        }
    }
    assert!(draw("<mxGraphModel><root></root></mxGraphModel>", Page::Index(0)).is_ok());
    assert!(draw("<mxGraphModel/>", Page::Index(0)).is_err());
}

#[test]
fn empty_diagram_gets_a_small_scene_and_a_warning() {
    let s = draw(
        "<mxGraphModel><root><mxCell id=\"0\"/><mxCell id=\"1\" parent=\"0\"/></root></mxGraphModel>",
        Page::Index(0),
    )
    .unwrap();
    assert!(s.items.is_empty());
    assert!(s.warnings.iter().any(|w| w.contains("empty")));
}

#[test]
fn page_background_is_drawn_first() {
    let src = r##"<mxGraphModel background="#f0f0f0"><root><mxCell id="0"/><mxCell id="1" parent="0"/><mxCell id="a" vertex="1" parent="1"><mxGeometry width="50" height="50" as="geometry"/></mxCell></root></mxGraphModel>"##;
    let s = scene(src);
    let Item::Shape(first) = &s.items[0] else { panic!() };
    assert_eq!(first.fill.as_ref().unwrap().color, "#f0f0f0ff");
    assert_eq!(first.bbox, [0.0, 0.0, s.size[0], s.size[1]]);
}

#[test]
fn rotation_turns_labels_with_the_shape() {
    let src = r##"<mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/><mxCell id="a" value="r" style="rotation=90;whiteSpace=wrap" vertex="1" parent="1"><mxGeometry x="0" y="0" width="100" height="20" as="geometry"/></mxCell></root></mxGraphModel>"##;
    let s = scene(src);
    let t = texts(&s)[0];
    assert_eq!(t.rotation, 90.0);
    let Item::Shape(sh) = &s.items[0] else { panic!() };
    assert_eq!(sh.rotation, 90.0);
}

#[test]
fn hostile_styles_produce_clean_scenes_at_the_source() {
    let xml = include_str!("../../../tests/e2e/drawio-hostile.drawio");
    let s = scene(xml);
    assert!(all_numbers(&s).iter().all(|v| v.is_finite()), "non-finite number in {s:?}");
    let ok = |c: &str| crate::color::is_valid_output(c);
    let paint_ok = |p: &Option<crate::scene::Paint>| {
        p.as_ref().is_none_or(|p| ok(&p.color) && p.gradient.as_ref().is_none_or(|g| ok(&g.color)))
    };
    for item in &s.items {
        match item {
            Item::Shape(sh) => {
                assert!(paint_ok(&sh.fill));
                assert!(sh.stroke.as_ref().is_none_or(|st| ok(&st.paint.color) && st.dash.iter().all(|d| d.is_finite())));
            }
            Item::Text(t) => {
                assert!(paint_ok(&t.background));
                assert!(t.runs.iter().all(|r| ok(&r.color) && r.size.is_finite() && r.font.as_deref() != Some("")));
            }
        }
    }
    // The script body is gone and the entity is decoded.
    assert!(texts(&s).iter().any(|t| plain(t).contains("huge") && plain(t).contains("caf\u{e9}") && !plain(t).contains("alert")));
}

#[test]
fn negative_sizes_do_not_panic_in_any_shape() {
    let styles = ["shape=cube", "shape=cylinder", "shape=note", "shape=folder", "shape=process", "shape=step", "swimlane", "rounded=1", "shape=hexagon", "shape=trapezoid", "shape=parallelogram", "shape=callout", "shape=tape", "shape=card", "triangle", "shape=cross"];
    for style in styles {
        let xml = format!(
            r#"<mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/><mxCell id="v" vertex="1" parent="1" style="{style};size=10"><mxGeometry x="0" y="0" width="-180" height="-40" as="geometry"/></mxCell></root></mxGraphModel>"#
        );
        draw(&xml, Page::Index(0)).unwrap_or_else(|e| panic!("{style}: {e}"));
    }
}
