//! Tests over the whole PlantUML pipeline using the fixtures in tests/fixtures/plantuml.
//! Text is "measured" with a made-up font (every character 0.55 em wide, lines 1.2 em
//! high), which is enough to check the layout invariants without Typst.

use super::model::Measured;
use super::*;
use crate::scene::{Item, Label};
use crate::test_util::{fixtures as shared_fixtures, render_with, sizes};

fn fixtures(prefix: &str) -> Vec<(String, String)> {
    shared_fixtures("plantuml", "puml", prefix)
}

fn render(src: &str) -> Result<Scene, String> {
    render_with(labels, layout, src)
}

#[test]
fn every_fixture_lays_out_deterministically() {
    for (name, src) in fixtures("") {
        let l1 = labels(src.as_bytes()).unwrap_or_else(|e| panic!("{name}: {e}"));
        let l2 = labels(src.as_bytes()).unwrap();
        assert_eq!(l1, l2, "{name}: label order changed between parses");
        let s = sizes(&l1);
        let a = layout(src.as_bytes(), &s).unwrap_or_else(|e| panic!("{name}: {e}"));
        let b = layout(src.as_bytes(), &s).unwrap();
        assert_eq!(a, b, "{name}: layout is not deterministic");
        assert!(a.size[0] > 10.0 && a.size[1] > 10.0, "{name}: empty scene");
        assert!(!a.items.is_empty());
        a.to_cbor().unwrap();
    }
}

#[test]
fn every_item_has_finite_coordinates_inside_the_scene() {
    for (name, src) in fixtures("") {
        let scene = render(&src).unwrap();
        let [w, h] = scene.size;
        for item in &scene.items {
            let (x0, y0, x1, y1) = match item {
                Item::Shape(s) => (s.bbox[0], s.bbox[1], s.bbox[0] + s.bbox[2], s.bbox[1] + s.bbox[3]),
                Item::Text(t) => (t.x, t.y, t.x + t.w, t.y + t.h),
            };
            assert!([x0, y0, x1, y1].iter().all(|v| v.is_finite()), "{name}: non-finite item");
            assert!(x0 >= -0.5 && y0 >= -0.5 && x1 <= w + 0.5 && y1 <= h + 0.5, "{name}: item ({x0},{y0},{x1},{y1}) outside {w}x{h}");
        }
    }
}

#[test]
fn wrong_number_of_sizes_is_an_error() {
    let src = "Alice -> Bob : hi";
    let n = labels(src.as_bytes()).unwrap().len();
    let e = layout(src.as_bytes(), &vec![(10.0, 10.0); n + 1]).unwrap_err();
    assert!(e.contains("label sizes"), "{e}");
    assert!(layout(src.as_bytes(), &[]).is_err());
}

#[test]
fn unsupported_types_name_the_type_and_line() {
    let cases = [
        ("object Foo\nFoo : x = 1", "line 1: object diagrams are not supported yet"),
        ("title t\nrobust \"Web\" as W\n@0\nW is Idle", "line 2: timing diagrams are not supported yet"),
        ("salt\n{\n  Login | \"name\"\n}", "line 1: salt (wireframe) diagrams are not supported yet"),
        ("entity A {\n  *id : int\n}\nA ||--o{ B", "line 4: entity relationship diagrams are not supported yet"),
        ("@startgantt\n[T] lasts 2 days\n@endgantt", "line 1: gantt diagrams are not supported yet"),
        ("@startmindmap\n* a\n@endmindmap", "line 1: mindmap diagrams are not supported yet"),
        ("(*) --> \"First\"\n\"First\" --> (*)", "line 1: legacy activity syntax is not supported yet, use the new syntax (start, :action;, stop)"),
        ("@startjson\n{\"a\": 1}\n@endjson", "line 1: json diagrams are not supported yet"),
    ];
    for (src, want) in cases {
        let e = labels(src.as_bytes()).unwrap_err();
        assert_eq!(e, want, "for {src:?}");
    }
}

#[test]
fn preprocessor_directives_are_errors_with_line_numbers() {
    let e = labels(b"A -> B\n!include other.puml\n").unwrap_err();
    assert_eq!(e, "line 2: the preprocessor directive !include is not supported");
    assert!(labels(b"!procedure foo()\n!endprocedure").unwrap_err().starts_with("line 1:"));
    let s = render("!theme cerulean\nA -> B").unwrap();
    assert!(s.warnings.iter().any(|w| w.starts_with("line 1:") && w.contains("!theme")), "{:?}", s.warnings);
}

#[test]
fn syntax_errors_name_the_line_after_continuations_and_comments() {
    let src = "@startuml\n' comment\nA -> B : a \\\n   b\n/' block\ncomment '/\nthis is nonsense\n@enduml";
    let e = labels(src.as_bytes()).unwrap_err();
    assert!(e.starts_with("line 7:"), "{e}");
}

#[test]
fn skinparam_handwritten_warns() {
    let s = render("skinparam handwritten true\nskinparam shadowing false\nA -> B").unwrap();
    assert_eq!(s.warnings.len(), 1, "{:?}", s.warnings);
}

#[test]
fn background_color_adds_a_backdrop() {
    let plain = render("A -> B").unwrap();
    let colored = render("skinparam backgroundColor #FFFFEE\nA -> B").unwrap();
    assert_eq!(colored.items.len(), plain.items.len() + 1);
}

#[test]
fn title_header_footer_legend_caption_are_laid_out() {
    let src = "title T\nheader H\nfooter F\ncaption C\nlegend right\nL\nendlegend\nA -> B";
    let plain = render("A -> B").unwrap();
    let full = render(src).unwrap();
    assert!(full.size[1] > plain.size[1] + 40.0);
    assert_eq!(labels(src.as_bytes()).unwrap().len(), labels(b"A -> B").unwrap().len() + 5);
}

// ---- sequence invariants ----------------------------------------------------------------

fn sequence(src: &str) -> (layout::sequence::SeqInfo, usize) {
    let parsed = parse(src.as_bytes()).unwrap();
    let Diagram::Sequence(m) = &parsed.diagram else { panic!("not a sequence diagram") };
    let measured = Measured::new(parsed.ctx.reg.labels.clone(), sizes(&parsed.ctx.reg.labels));
    let (_, info) = layout::sequence::layout_info(m, &parsed.ctx.skin, &measured);
    (info, m.participants.len())
}

#[test]
fn sequence_participants_do_not_overlap_and_messages_descend() {
    for (name, src) in fixtures("seq_") {
        let (info, n) = sequence(&src);
        assert_eq!(info.heads.len(), n, "{name}");
        for w in info.heads.windows(2) {
            assert!(w[0].1 <= w[1].0 + 0.01, "{name}: participant boxes overlap: {:?}", info.heads);
        }
        let mut last = f32::MIN;
        for m in &info.messages {
            assert!(m.y > last, "{name}: message at y={} is not below the previous one ({last})", m.y);
            last = m.y;
        }
    }
}

#[test]
fn sequence_message_labels_fit_between_their_lifelines() {
    for (name, src) in fixtures("seq_") {
        let (info, _) = sequence(&src);
        for m in info.messages.iter().filter(|m| !m.is_self) {
            let Some((l0, l1)) = m.label else { continue };
            let (a, b) = (m.from_x.min(m.to_x), m.from_x.max(m.to_x));
            // Centered or right aligned labels may be narrower than the arrow; none may
            // stick out of it.
            assert!(l0 >= a - 0.5 && l1 <= b + 0.5, "{name}: label ({l0},{l1}) does not fit between ({a},{b})");
        }
    }
}

#[test]
fn sequence_column_distance_grows_with_label_width() {
    let short = sequence("A -> B : x").0;
    let long = sequence("A -> B : a very very long label indeed that needs room").0;
    let gap = |i: &layout::sequence::SeqInfo| i.heads[1].0 - i.heads[0].1;
    assert!(gap(&long) > gap(&short) + 100.0);
}

#[test]
fn sequence_order_alias_and_auto_creation() {
    let (info, n) = sequence("participant A order 20\nparticipant B order 10\nA -> C");
    assert_eq!(n, 3);
    assert_eq!(info.messages.len(), 1);
}

#[test]
fn sequence_groups_widen_to_fit_headers() {
    let narrow = render("alt a very long condition text that is wider than the participants\nA -> B : x\nend").unwrap();
    let plain = render("A -> B : x").unwrap();
    assert!(narrow.size[0] > plain.size[0]);
}

#[test]
fn sequence_labels_are_registered_in_source_order() {
    let l = labels(b"title T\nA -> B : first\nnote over A : second\nB --> A : third").unwrap();
    let text: Vec<String> = l.iter().map(|l| creole::plain(&l.runs)).collect();
    assert_eq!(&text[..4], ["T", "first", "second", "third"]);
}

// ---- activity invariants ----------------------------------------------------------------

fn activity(src: &str) -> layout::flow::FlowInfo {
    let parsed = parse(src.as_bytes()).unwrap();
    let Diagram::Activity(m) = &parsed.diagram else { panic!("not an activity diagram") };
    let measured = Measured::new(parsed.ctx.reg.labels.clone(), sizes(&parsed.ctx.reg.labels));
    layout::flow::layout_info(m, &parsed.ctx.skin, &measured).1
}

fn overlap(a: &[f32; 4], b: &[f32; 4]) -> bool {
    a[0] < b[2] - 0.01 && b[0] < a[2] - 0.01 && a[1] < b[3] - 0.01 && b[1] < a[3] - 0.01
}

#[test]
fn activity_nodes_do_not_overlap() {
    for (name, src) in fixtures("act_") {
        let info = activity(&src);
        assert!(!info.tiles.is_empty(), "{name}");
        for (i, a) in info.tiles.iter().enumerate() {
            for b in &info.tiles[i + 1..] {
                assert!(!overlap(a, b), "{name}: nodes overlap: {a:?} and {b:?}");
            }
        }
    }
}

fn on_border(p: (f32, f32), t: &[f32; 4]) -> bool {
    let eps = 0.6;
    let inside_x = p.0 >= t[0] - eps && p.0 <= t[2] + eps;
    let inside_y = p.1 >= t[1] - eps && p.1 <= t[3] + eps;
    inside_x && inside_y && ((p.0 - t[0]).abs() < eps || (p.0 - t[2]).abs() < eps || (p.1 - t[1]).abs() < eps || (p.1 - t[3]).abs() < eps)
}

fn on_line(p: (f32, f32), line: &[(f32, f32)]) -> bool {
    line.windows(2).any(|w| {
        let (x0, x1) = (w[0].0.min(w[1].0) - 0.6, w[0].0.max(w[1].0) + 0.6);
        let (y0, y1) = (w[0].1.min(w[1].1) - 0.6, w[0].1.max(w[1].1) + 0.6);
        p.0 >= x0 && p.0 <= x1 && p.1 >= y0 && p.1 <= y1
    })
}

#[test]
fn activity_arrows_end_on_node_borders() {
    for (name, src) in fixtures("act_") {
        let info = activity(&src);
        for (pts, head) in &info.arrows {
            if !head || pts.len() < 2 {
                continue;
            }
            let tip = *pts.last().unwrap();
            // A `split` joins its branches on a bar that is only a line.
            let on_bus = info.arrows.iter().any(|(o, _)| !std::ptr::eq(o, pts) && on_line(tip, o));
            assert!(info.tiles.iter().any(|t| on_border(tip, t)) || on_bus, "{name}: arrow ending at {tip:?} does not touch a node");
            // Arrows start at a node, or on another line (a split's bar, a branch that
            // skips over a node).
            let start = pts[0];
            let near_arrow = info.arrows.iter().any(|(o, _)| !std::ptr::eq(o, pts) && on_line(start, o));
            assert!(info.tiles.iter().any(|t| on_border(start, t)) || near_arrow, "{name}: arrow starting at {start:?} is not attached");
        }
    }
}

#[test]
fn activity_flows_top_down() {
    let info = activity("start\n:a;\n:b;\n:c;\nstop");
    let ys: Vec<f32> = info.tiles.iter().map(|t| t[1]).collect();
    assert!(ys.windows(2).all(|w| w[0] < w[1]), "{ys:?}");
}

#[test]
fn activity_swimlanes_get_their_own_columns() {
    let info = activity("|A|\n:x;\n|B|\n:y;");
    assert!(info.tiles[0][2] < info.tiles[1][0], "{:?}", info.tiles);
}

#[test]
fn activity_branches_sit_side_by_side() {
    let info = activity("if (c?) then (yes)\n:left;\nelse (no)\n:right;\nendif");
    // diamond, left, right, merge
    assert_eq!(info.tiles.len(), 4);
    assert!(info.tiles[1][2] < info.tiles[2][0]);
}

#[test]
fn activity_label_order_follows_source() {
    let l = labels(b"start\n:one;\nif (two?) then (three)\n:four;\nelse (five)\n:six;\nendif\nstop").unwrap();
    let text: Vec<String> = l.iter().map(|l| creole::plain(&l.runs)).collect();
    assert_eq!(text, ["one", "two?", "three", "four", "five", "six"]);
}

// ---- robustness ------------------------------------------------------------------------------

#[test]
fn truncated_and_mangled_fixtures_never_panic() {
    // A panic in the wasm module is an unexplained trap for the user, so every input
    // has to end in either a scene or a message.
    for (name, src) in fixtures("") {
        let lines: Vec<&str> = src.lines().collect();
        for cut in 0..=lines.len() {
            let head = lines[..cut].join("\n");
            let _ = render(&head);
        }
        for skip in 0..lines.len() {
            let mut l = lines.clone();
            l.remove(skip);
            let _ = render(&l.join("\n"));
        }
        for (i, line) in lines.iter().enumerate() {
            // Chop each line in two places.
            for at in [line.len() / 3, 2 * line.len() / 3] {
                if line.is_char_boundary(at) {
                    let mut l = lines.clone();
                    let chopped = line[..at].to_string();
                    l[i] = &chopped;
                    let _ = render(&l.join("\n"));
                }
            }
        }
        let _ = name;
    }
}

// ---- graph diagram invariants ---------------------------------------------------------

use layout::gdraw::GraphInfo;
use layout::graph::Rect;
use layout::lint::{enters, rect_to_poly};

/// Layout info with the parent of every entity and the ends of every link.
struct G {
    info: GraphInfo,
    parents: Vec<Option<usize>>,
    links: Vec<(usize, usize)>,
}

fn graph_with(src: &str, sizes_fn: impl Fn(&[Label]) -> Vec<(f32, f32)>) -> G {
    let parsed = parse(src.as_bytes()).unwrap();
    let measured = Measured::new(parsed.ctx.reg.labels.clone(), sizes_fn(&parsed.ctx.reg.labels));
    let skin = &parsed.ctx.skin;
    match &parsed.diagram {
        Diagram::Class(m) => G { info: layout::class::layout_info(m, skin, &measured).unwrap().1, parents: m.ents.iter().map(|e| e.parent).collect(), links: m.links.iter().map(|l| (l.from, l.to)).collect() },
        Diagram::State(m) => G { info: layout::state::layout_info(m, skin, &measured).unwrap().1, parents: m.ents.iter().map(|e| e.parent).collect(), links: m.links.iter().map(|l| (l.from, l.to)).collect() },
        Diagram::Description(m) => G { info: layout::description::layout_info(m, skin, &measured).unwrap().1, parents: m.ents.iter().map(|e| e.parent).collect(), links: m.links.iter().map(|l| (l.from, l.to)).collect() },
        _ => panic!("not a graph diagram"),
    }
}

fn graph(src: &str) -> G {
    graph_with(src, sizes)
}

fn graph_fixtures() -> Vec<(String, String)> {
    ["cls_", "st_", "uc_", "cmp_"].iter().flat_map(|p| fixtures(p)).collect()
}

fn inside(inner: &Rect, outer: &Rect) -> bool {
    let e = 0.01;
    inner.x >= outer.x - e && inner.y >= outer.y - e && inner.right() <= outer.right() + e && inner.bottom() <= outer.bottom() + e
}

fn rects_overlap(a: &Rect, b: &Rect) -> bool {
    a.x < b.right() - 0.01 && b.x < a.right() - 0.01 && a.y < b.bottom() - 0.01 && b.y < a.bottom() - 0.01
}

fn is_ancestor(parents: &[Option<usize>], anc: usize, mut of: usize) -> bool {
    while let Some(p) = parents[of] {
        if p == anc {
            return true;
        }
        of = p;
    }
    false
}

#[test]
fn graph_nodes_do_not_overlap() {
    for (name, src) in graph_fixtures() {
        let g = graph(&src);
        for (i, (ea, a)) in g.info.nodes.iter().enumerate() {
            for (eb, b) in &g.info.nodes[i + 1..] {
                assert!(!rects_overlap(a, b), "{name}: entities {ea} and {eb} overlap: {a:?} {b:?}");
            }
        }
    }
}

#[test]
fn clusters_hold_exactly_their_members() {
    for (name, src) in graph_fixtures() {
        let g = graph(&src);
        for (c, cr) in &g.info.clusters {
            for (n, nr) in &g.info.nodes {
                if is_ancestor(&g.parents, *c, *n) {
                    assert!(inside(nr, cr), "{name}: entity {n} sticks out of its container {c}: {nr:?} vs {cr:?}");
                } else {
                    assert!(!rects_overlap(nr, cr), "{name}: entity {n} overlaps container {c} it does not belong to");
                }
            }
            for (d, dr) in &g.info.clusters {
                if c == d {
                    continue;
                }
                if is_ancestor(&g.parents, *c, *d) {
                    assert!(inside(dr, cr), "{name}: container {d} sticks out of {c}");
                } else if !is_ancestor(&g.parents, *d, *c) {
                    assert!(!rects_overlap(cr, dr), "{name}: containers {c} and {d} overlap");
                }
            }
        }
    }
}

/// Distance from `p` to the outline of `r`; zero on it, positive inside and outside.
fn outline_distance(r: &Rect, p: (f32, f32)) -> f32 {
    let dx = (r.x - p.0).max(p.0 - r.right()).max(0.0);
    let dy = (r.y - p.1).max(p.1 - r.bottom()).max(0.0);
    if dx > 0.0 || dy > 0.0 {
        return (dx * dx + dy * dy).sqrt();
    }
    (p.0 - r.x).abs().min((p.0 - r.right()).abs()).min((p.1 - r.y).abs()).min((p.1 - r.bottom()).abs())
}

#[test]
fn graph_edges_end_on_their_nodes() {
    for (name, src) in graph_fixtures() {
        let g = graph(&src);
        let rect_of = |e: usize| g.info.nodes.iter().chain(&g.info.clusters).find(|(i, _)| *i == e).map(|(_, r)| *r);
        for (li, route) in &g.info.routes {
            let (from, to) = g.links[*li];
            if from == to {
                continue;
            }
            for (ent, p) in [(from, route.start()), (to, route.end())] {
                let Some(r) = rect_of(ent) else { continue };
                // Class boxes and containers are rectangles, so the line has to meet the
                // outline; ellipses and diamonds only touch their box at a few points.
                let tolerance = 1.0;
                assert!(r.x - tolerance <= p.0 && p.0 <= r.right() + tolerance && r.y - tolerance <= p.1 && p.1 <= r.bottom() + tolerance, "{name}: link {li} ends at {p:?}, outside {r:?}");
                if name.starts_with("cls_") || g.info.clusters.iter().any(|(c, _)| *c == ent) {
                    assert!(outline_distance(&r, p) <= tolerance, "{name}: link {li} ends at {p:?}, not on the outline of {r:?}");
                }
            }
        }
    }
}

#[test]
fn graph_routes_are_continuous_and_finite() {
    for (name, src) in graph_fixtures() {
        let g = graph(&src);
        for (li, route) in &g.info.routes {
            assert!(!route.segs.is_empty(), "{name}: link {li} has no route");
            for pair in route.segs.windows(2) {
                assert!(near(pair[0][3], pair[1][0]), "{name}: link {li} has a gap in its route");
            }
            assert!(route.segs.iter().flatten().all(|p| p.0.is_finite() && p.1.is_finite()), "{name}: link {li} has a non-finite point");
        }
    }
}

fn centers(g: &G, keys: &[usize]) -> Vec<(f32, f32)> {
    keys.iter().map(|k| g.info.nodes.iter().find(|(i, _)| i == k).map(|(_, r)| (r.x + r.w / 2.0, r.y + r.h / 2.0)).unwrap()).collect()
}

#[test]
fn direction_hints_place_nodes() {
    let g = graph("class A\nclass B\nclass C\nclass D\nclass E\nA -up-> B\nA -down-> C\nA -left-> D\nA -right-> E");
    let c = centers(&g, &[0, 1, 2, 3, 4]);
    assert!(c[1].1 < c[0].1 - 20.0, "up");
    assert!(c[2].1 > c[0].1 + 20.0, "down");
    assert!(c[3].0 < c[0].0 - 20.0 && (c[3].1 - c[0].1).abs() < 1.0, "left");
    assert!(c[4].0 > c[0].0 + 20.0 && (c[4].1 - c[0].1).abs() < 1.0, "right");
}

#[test]
fn up_and_left_routes_still_run_from_the_first_entity_to_the_second() {
    let g = graph("class A\nclass B\nclass C\nA -up-> B\nA -left-> C");
    let rect = |e: usize| g.info.nodes.iter().find(|(i, _)| *i == e).unwrap().1;
    for (li, route) in &g.info.routes {
        let (from, to) = g.links[*li];
        let (a, b) = (rect(from), rect(to));
        assert!(outline_distance(&a, route.start()) < 1.0 && outline_distance(&b, route.end()) < 1.0, "link {li} runs backwards");
    }
}

#[test]
fn arrow_length_decides_horizontal_or_vertical() {
    let g = graph("class A\nclass B\nclass C\nclass D\nA -> B\nA --> C\nA ---> D");
    let c = centers(&g, &[0, 1, 2, 3]);
    assert!((c[1].1 - c[0].1).abs() < 1.0 && c[1].0 > c[0].0, "one dash is horizontal");
    assert!(c[2].1 > c[0].1 + 20.0, "two dashes go down");
    assert!(c[3].1 > c[2].1, "three dashes go further down");
}

#[test]
fn left_to_right_direction_runs_ranks_sideways() {
    let g = graph("left to right direction\nactor A\nusecase B\nA --> B");
    let c = centers(&g, &[0, 1]);
    assert!(c[1].0 > c[0].0 + 20.0 && (c[1].1 - c[0].1).abs() < 40.0);
    let g = graph("left to right direction\n[*] --> A\nA --> B");
    let c = centers(&g, &[0, 1, 2]);
    assert!(c[0].0 < c[1].0 && c[1].0 < c[2].0);
}

#[test]
fn class_boxes_grow_with_their_members() {
    let small = graph("class A").info.nodes[0].1;
    let big = graph("class A {\n  +aVeryLongMethodName(withArguments : String)\n  field1\n  field2\n  field3\n}").info.nodes[0].1;
    assert!(big.h > small.h + 40.0 && big.w > small.w + 100.0);
}

#[test]
fn state_regions_are_stacked() {
    let g = graph("state P {\n  state A\n  --\n  state B\n}");
    let c = centers(&g, &[1, 4]);
    assert!(c[1].1 > c[0].1 + 20.0);
    let g = graph("state P {\n  state A\n  ||\n  state B\n}");
    let c = centers(&g, &[1, 4]);
    assert!(c[1].0 > c[0].0 + 20.0);
}

#[test]
fn edges_into_a_concurrent_region_reach_the_state_inside() {
    let g = graph("state P {\n  state A\n  --\n  state B\n}\n[*] --> B");
    let b = g.info.nodes.iter().find(|(i, _)| g.info.routes.iter().any(|(li, _)| g.links[*li].1 == *i)).map(|(_, r)| *r).unwrap();
    let (_, route) = &g.info.routes[0];
    assert!(outline_distance(&b, route.end()) < 1.0, "{route:?} vs {b:?}");
}

#[test]
fn linetype_ortho_gives_axis_aligned_routes() {
    let g = graph("skinparam linetype ortho\nclass A\nA --> B\nA --> C\nB --> D\nC --> D\nA --> D");
    for (_, r) in &g.info.routes {
        for s in &r.segs {
            let flat_x = s.iter().all(|p| (p.0 - s[0].0).abs() < 0.5);
            let flat_y = s.iter().all(|p| (p.1 - s[0].1).abs() < 0.5);
            assert!(flat_x || flat_y, "{s:?}");
        }
    }
}

fn big_class_source(n: usize, m: usize) -> String {
    let mut out = String::new();
    for i in 0..n {
        out += &format!("class C{i} {{\n  -f{i} : int\n  +m{i}()\n}}\n");
    }
    let mut seed = 12345u64;
    let mut next = |k: usize| {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((seed >> 33) as usize) % k
    };
    for _ in 0..m {
        let (a, b) = (next(n), next(n));
        let arrow = ["-->", "<|--", "*--", "..>", "o--"][next(5)];
        out += &format!("C{a} {arrow} C{b}\n");
    }
    out
}

/// A hundred classes with a random web of relations: it has to finish with every box
/// placed. The time limit only catches a hang; use `benchmark_hundred_classes` to look
/// at real numbers.
#[test]
fn a_hundred_classes_lay_out() {
    let src = big_class_source(100, 150);
    let t = std::time::Instant::now();
    let l = labels(src.as_bytes()).unwrap();
    let scene = layout(src.as_bytes(), &sizes(&l)).unwrap();
    assert!(t.elapsed().as_secs() < 60, "took {:?}", t.elapsed());
    assert!(scene.size[0] > 500.0);
}

#[test]
#[ignore = "timing only: cargo test --profile test-opt --lib benchmark_hundred_classes -- --ignored --nocapture"]
fn benchmark_hundred_classes() {
    let src = big_class_source(100, 150);
    let l = labels(src.as_bytes()).unwrap();
    let s = sizes(&l);
    let t = std::time::Instant::now();
    for _ in 0..5 {
        layout(src.as_bytes(), &s).unwrap();
    }
    println!("100 classes, 150 relations: {:?} per layout", t.elapsed() / 5);
}

#[test]
fn graph_diagrams_register_labels_in_source_order() {
    let l = labels(b"title T\nclass A\nclass B\nA --> B : first\nnote right of A : second").unwrap();
    let text: Vec<String> = l.iter().map(|l| creole::plain(&l.runs)).collect();
    assert_eq!(text, ["T", "A", "B", "first", "second"]);
}

fn entity_within(g: &G, e: usize, c: usize) -> bool {
    e == c || is_ancestor(&g.parents, c, e)
}

fn near(a: (f32, f32), b: (f32, f32)) -> bool {
    (a.0 - b.0).abs() < 0.5 && (a.1 - b.1).abs() < 0.5
}

#[test]
fn graph_edges_never_share_a_route() {
    for (name, src) in graph_fixtures() {
        let g = graph(&src);
        let routes: Vec<_> = g.info.routes.iter().filter(|(li, _)| g.links[*li].0 != g.links[*li].1).collect();
        for (i, (la, a)) in routes.iter().enumerate() {
            for (lb, b) in &routes[i + 1..] {
                if a.segs.len() != b.segs.len() {
                    continue;
                }
                let (pa, pb) = (a.flatten(), b.flatten());
                let same = pa.iter().zip(&pb).all(|(p, q)| near(*p, *q));
                let reversed = pa.iter().zip(pb.iter().rev()).all(|(p, q)| near(*p, *q));
                assert!(!same && !reversed, "{name}: links {la} and {lb} are drawn on top of each other");
            }
        }
    }
}

#[test]
fn graph_edges_stay_clear_of_other_nodes() {
    let mut bad = Vec::new();
    for (name, src) in graph_fixtures() {
        let g = graph(&src);
        for (li, route) in &g.info.routes {
            let (from, to) = g.links[*li];
            if from == to {
                continue;
            }
            let poly = route.flatten();
            for (n, r) in &g.info.nodes {
                if *n == from || *n == to || is_ancestor(&g.parents, from, *n) || is_ancestor(&g.parents, to, *n) {
                    continue;
                }
                let core = Rect::new(r.x + 3.0, r.y + 3.0, r.w - 6.0, r.h - 6.0);
                if enters(&poly, &core) {
                    bad.push(format!("{name}: link {li} ({from} -> {to}) runs through entity {n}"));
                }
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn graph_labels_do_not_overlap_nodes_or_each_other() {
    let mut bad = Vec::new();
    for (name, src) in graph_fixtures() {
        let g = graph(&src);
        let shrink = |r: &Rect| Rect::new(r.x + 0.5, r.y + 0.5, r.w - 1.0, r.h - 1.0);
        for (i, (li, _, r)) in g.info.labels.iter().enumerate() {
            for (n, nr) in &g.info.nodes {
                if rects_overlap(&shrink(r), nr) {
                    bad.push(format!("{name}: a label of link {li} overlaps entity {n}"));
                }
            }
            for (lj, _, other) in &g.info.labels[i + 1..] {
                if rects_overlap(&shrink(r), &shrink(other)) {
                    bad.push(format!("{name}: a label of link {li} overlaps a label of link {lj}"));
                }
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn graph_edges_inside_a_container_stay_inside() {
    let mut bad = Vec::new();
    for (name, src) in graph_fixtures() {
        let g = graph(&src);
        for (li, route) in &g.info.routes {
            let (from, to) = g.links[*li];
            if from == to {
                continue;
            }
            let poly = route.flatten();
            for (c, cr) in &g.info.clusters {
                if from == *c || to == *c || !entity_within(&g, from, *c) || !entity_within(&g, to, *c) {
                    continue;
                }
                let room = Rect::new(cr.x - 2.0, cr.y - 2.0, cr.w + 4.0, cr.h + 4.0);
                if !poly.iter().all(|p| p.0 >= room.x && p.0 <= room.right() && p.1 >= room.y && p.1 <= room.bottom()) {
                    bad.push(format!("{name}: link {li} leaves container {c} that holds both its ends"));
                }
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn graph_labels_do_not_sit_on_their_own_line() {
    let mut bad = Vec::new();
    for (name, src) in graph_fixtures() {
        let g = graph(&src);
        let polys: Vec<_> = g.info.routes.iter().map(|(li, r)| (*li, r.flatten())).collect();
        for (li, end, r) in &g.info.labels {
            let probe = Rect::new(r.x + 1.5, r.y + 1.5, r.w - 3.0, r.h - 3.0);
            for (lj, poly) in &polys {
                // Dot keeps a label clear of its own line but not of unrelated ones.
                if lj == li && enters(poly, &probe) {
                    bad.push(format!("{name}: a {}label of link {li} is crossed by link {lj}", if *end { "multiplicity " } else { "" }));
                }
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn graph_edge_labels_sit_next_to_their_own_edge() {
    let mut bad = Vec::new();
    for (name, src) in graph_fixtures() {
        let g = graph(&src);
        for (li, _, r) in g.info.labels.iter().filter(|(_, end, _)| !end) {
            let Some((_, route)) = g.info.routes.iter().find(|(lj, _)| lj == li) else { continue };
            let d = rect_to_poly(r, &route.flatten());
            if d > 12.0 {
                bad.push(format!("{name}: the label of link {li} is {d:.0} pt away from its edge"));
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn graph_end_labels_clear_every_edge() {
    let mut bad = Vec::new();
    for (name, src) in graph_fixtures() {
        let g = graph(&src);
        for (li, _, r) in g.info.labels.iter().filter(|(_, end, _)| *end) {
            let probe = Rect::new(r.x + 1.5, r.y + 1.5, r.w - 3.0, r.h - 3.0);
            for (lj, route) in &g.info.routes {
                if enters(&route.flatten(), &probe) {
                    bad.push(format!("{name}: a multiplicity of link {li} is crossed by link {lj}"));
                }
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
#[test]
fn skinparam_nodesep_and_ranksep_set_the_gaps() {
    let gap = |extra: &str| {
        let g = graph(&format!("{extra}\nclass A\nclass B\nclass C\nA --> B\nA --> C"));
        let r = |i: usize| g.info.nodes.iter().find(|(n, _)| *n == i).unwrap().1;
        (r(2).x - r(1).right(), r(1).y - r(0).bottom())
    };
    let (side, down) = gap("");
    let (wide_side, wide_down) = gap("skinparam nodesep 120\nskinparam ranksep 150");
    assert!(wide_side > side + 60.0, "nodesep: {side} -> {wide_side}");
    assert!(wide_down > down + 60.0, "ranksep: {down} -> {wide_down}");
}
