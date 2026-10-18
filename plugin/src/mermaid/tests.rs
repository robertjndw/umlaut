use super::*;
use crate::test_util::{fixtures, render_with};

fn err_of(src: &str) -> String {
    labels(src.as_bytes()).unwrap_err()
}

#[test]
fn unsupported_types_name_the_type_and_line() {
    assert_eq!(
        err_of("%% c\ngantt\n  title x"),
        "line 2: Mermaid gantt diagrams are not supported yet (supported: flowchart, sequenceDiagram, classDiagram, stateDiagram)"
    );
    assert!(err_of("erDiagram\nA ||--o{ B : has").contains("Mermaid erDiagram diagrams"));
    assert!(err_of("\n\nwat").starts_with("line 3: expected a diagram type such as `flowchart TD`"));
    assert!(err_of("   ").contains("empty"));
    assert!(layout(b"gantt", &[]).is_err());
}

#[test]
fn an_empty_diagram_of_every_type_is_an_error() {
    for (src, want) in [
        ("flowchart TD", "line 1: Mermaid flowcharts need at least one node"),
        ("sequenceDiagram", "line 1: Mermaid sequence diagrams need at least one participant"),
        ("classDiagram", "line 1: Mermaid class diagrams need at least one class"),
        ("stateDiagram-v2", "line 1: Mermaid state diagrams need at least one state"),
    ] {
        assert_eq!(err_of(src), want, "{src}");
    }
}

/// Every fixture of every type, through both passes with the fake font.
fn all_fixtures() -> Vec<(String, String)> {
    ["flow_", "seq_", "cls_", "st_"].iter().flat_map(|p| fixtures("mermaid", "mmd", p)).collect()
}

#[test]
fn every_fixture_lays_out_deterministically() {
    for (name, src) in all_fixtures() {
        let a = labels(src.as_bytes()).unwrap_or_else(|e| panic!("{name}: {e}"));
        let b = labels(src.as_bytes()).unwrap();
        assert_eq!(a, b, "{name}: labels differ between runs");
        let s1 = render_with(labels, layout, &src).unwrap_or_else(|e| panic!("{name}: {e}"));
        let s2 = render_with(labels, layout, &src).unwrap();
        assert_eq!(s1.to_cbor().unwrap(), s2.to_cbor().unwrap(), "{name}: scenes differ between runs");
        assert!(s1.size[0] > 0.0 && s1.size[1] > 0.0, "{name}: empty scene");
    }
}

#[test]
fn every_fixture_stays_inside_its_scene() {
    for (name, src) in all_fixtures() {
        let s = render_with(labels, layout, &src).unwrap();
        for it in &s.items {
            let b = match it {
                crate::scene::Item::Shape(sh) => sh.bbox,
                crate::scene::Item::Text(t) => [t.x, t.y, t.w, t.h],
            };
            assert!(b.iter().all(|v| v.is_finite()), "{name}: non-finite item {b:?}");
            assert!(b[0] >= -1.0 && b[1] >= -1.0 && b[0] + b[2] <= s.size[0] + 1.0 && b[1] + b[3] <= s.size[1] + 1.0, "{name}: item {b:?} outside {:?}", s.size);
        }
    }
}

#[test]
fn a_wrong_number_of_sizes_is_an_error_for_every_type() {
    for src in ["flowchart LR\nA-->B", "sequenceDiagram\nA->>B: hi", "classDiagram\nA <|-- B", "stateDiagram-v2\n[*] --> A"] {
        let n = labels(src.as_bytes()).unwrap().len();
        let e = layout(src.as_bytes(), &vec![(10.0, 10.0); n + 1]).unwrap_err();
        assert!(e.contains("label sizes"), "{src}: {e}");
    }
}

#[test]
fn front_matter_title_is_the_first_label() {
    let f = front(b"---\ntitle: Orders\n---\nflowchart LR\nA-->B").unwrap();
    assert_eq!(f.ctx.reg.labels.len(), 1);
    assert_eq!(f.ctx.meta.title, Some(crate::plantuml::model::LabelId(0)));
    let r = &f.ctx.reg.labels[0].runs;
    assert_eq!(r[0].text, "Orders");
    assert_eq!(r[0].size, 18.0);
    assert!(front(b"flowchart LR").unwrap().ctx.reg.labels.is_empty());
}

#[test]
fn a_title_statement_does_not_replace_the_front_matter_title() {
    let mut f = front(b"---\ntitle: One\n---\nsequenceDiagram").unwrap();
    set_title(&mut f.ctx, "Two");
    assert_eq!(f.ctx.reg.labels.len(), 1);
    let mut g = front(b"sequenceDiagram").unwrap();
    set_title(&mut g.ctx, "Two");
    assert_eq!(g.ctx.reg.labels.len(), 1);
}

#[test]
fn lexer_warnings_reach_the_context() {
    let f = front(b"%%{init: {}}%%\nflowchart TD").unwrap();
    assert_eq!(f.ctx.warnings, vec!["line 1: %%{init}%% directives are ignored".to_string()]);
    assert!(!f.header.wrap);
    let w = front(b"%%{init: {\"sequence\": {\"wrap\": true}}}%%\nsequenceDiagram").unwrap();
    assert!(w.header.wrap);
}

#[test]
fn size_count_mismatch_is_an_error() {
    let e = layout(b"flowchart LR\nA-->B", &[(1.0, 1.0); 3]).unwrap_err();
    assert_eq!(e, "the plugin expected 2 label sizes but got 3; the source changed between the two passes");
}
