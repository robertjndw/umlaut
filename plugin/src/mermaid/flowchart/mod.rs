//! Mermaid flowcharts: `parse` fills the model below, `layout` draws it with the
//! Graphviz-backed graph layout PlantUML's component diagrams use.
//!
//! Subgraphs are clusters and nodes mentioned inside one belong to it, so an entity's
//! parent can come later in the list than the entity itself; `layout` hands out all the
//! graph slots before filling them in.

mod layout;
mod parse;

pub use layout::{layout, layout_info};
pub use parse::parse;

use crate::mermaid::css::Css;
use crate::plantuml::diagrams::gcommon::{Ent, Link};
use crate::plantuml::layout::graph::RankDir;
use crate::plantuml::model::LabelId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// `A[text]`
    Rect,
    /// `A(text)`
    Round,
    /// `A([text])`
    Stadium,
    /// `A[[text]]`
    Subroutine,
    /// `A[(text)]`
    Cylinder,
    /// `A((text))`
    Circle,
    /// `A(((text)))`
    DoubleCircle,
    /// `A>text]`
    Asymmetric,
    /// `A{text}`
    Rhombus,
    /// `A{{text}}`
    Hexagon,
    /// `A[/text/]`
    LeanRight,
    /// `A[\text\]`
    LeanLeft,
    /// `A[/text\]`, wider at the bottom.
    Trapezoid,
    /// `A[\text/]`, wider at the top.
    TrapezoidAlt,
}

#[derive(Debug, Clone)]
pub struct Node {
    /// `None` for an empty label.
    pub label: Option<LabelId>,
    pub shape: Shape,
    /// `classDef`s, `class` and `style` resolved into one.
    pub css: Css,
}

#[derive(Debug, Clone)]
pub struct Subgraph {
    pub title: Option<LabelId>,
    pub css: Css,
}

#[derive(Debug, Clone)]
pub enum Def {
    Node(Node),
    Subgraph(Subgraph),
}

#[derive(Debug)]
pub struct Model {
    pub ents: Vec<Ent<Def>>,
    pub links: Vec<Link>,
    pub dir: RankDir,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mermaid::{labels, layout as mlayout, Diagram};
    use crate::plantuml::diagrams::gcommon::Deco;
    use crate::plantuml::model::Measured;
    use crate::test_util::{fixtures, render_with, sizes};

    fn parsed(src: &str) -> (Model, crate::plantuml::model::Ctx) {
        let p = crate::mermaid::parse(src.as_bytes()).unwrap();
        match p.diagram {
            Diagram::Flowchart(m) => (m, p.ctx),
            _ => panic!("not a flowchart"),
        }
    }

    fn model(src: &str) -> Model {
        parsed(src).0
    }

    fn fails(src: &str) -> String {
        labels(src.as_bytes()).unwrap_err()
    }

    fn keys(m: &Model) -> Vec<&str> {
        m.ents.iter().map(|e| e.key.as_str()).collect()
    }

    fn shape_of(m: &Model, key: &str) -> Shape {
        match &m.ents.iter().find(|e| e.key == key).unwrap().def {
            Def::Node(n) => n.shape,
            _ => panic!("{key} is not a node"),
        }
    }

    fn idx(m: &Model, key: &str) -> usize {
        m.ents.iter().position(|e| e.key == key).unwrap()
    }

    #[test]
    fn direction_comes_from_the_header() {
        assert_eq!(model("flowchart TD\nA-->B").dir, RankDir::TB);
        assert_eq!(model("graph RL\nA-->B").dir, RankDir::RL);
        assert_eq!(model("flowchart BT\nA-->B").dir, RankDir::BT);
        assert!(model("flowchart\nA-->B").links.len() == 1);
    }

    #[test]
    fn every_shape_delimiter() {
        let m = model(
            "flowchart TD\nA[a]\nB(b)\nC([c])\nD[[d]]\nE[(e)]\nF((f))\nG(((g)))\nH>h]\nI{i}\nJ{{j}}\nK[/k/]\nL[\\l\\]\nM[/m\\]\nN[\\n/]",
        );
        let want = [
            Shape::Rect,
            Shape::Round,
            Shape::Stadium,
            Shape::Subroutine,
            Shape::Cylinder,
            Shape::Circle,
            Shape::DoubleCircle,
            Shape::Asymmetric,
            Shape::Rhombus,
            Shape::Hexagon,
            Shape::LeanRight,
            Shape::LeanLeft,
            Shape::Trapezoid,
            Shape::TrapezoidAlt,
        ];
        for (k, s) in "ABCDEFGHIJKLMN".chars().zip(want) {
            assert_eq!(shape_of(&m, &k.to_string()), s, "{k}");
        }
    }

    #[test]
    fn labels_may_hold_delimiters_when_quoted() {
        let (m, ctx) = parsed("flowchart TD\nA[\"text (with) [brackets]\"] --> B");
        assert_eq!(m.ents.len(), 2);
        assert_eq!(ctx.reg.labels[0].runs[0].text, "text (with) [brackets]");
        assert_eq!(ctx.reg.labels[1].runs[0].text, "B");
    }

    #[test]
    fn v11_shape_syntax() {
        let (m, ctx) = parsed("flowchart TD\nA@{ shape: diam, label: \"Pick\" }\nB@{ shape: cyl }\nC@{\n shape: hex\n label: \"Multi\"\n}\nD@{ shape: curv-trap }");
        assert_eq!(shape_of(&m, "A"), Shape::Rhombus);
        assert_eq!(shape_of(&m, "B"), Shape::Cylinder);
        assert_eq!(shape_of(&m, "C"), Shape::Hexagon);
        assert_eq!(shape_of(&m, "D"), Shape::Rect);
        assert_eq!(ctx.reg.labels[0].runs[0].text, "Pick");
        assert_eq!(ctx.reg.labels[1].runs[0].text, "B");
        assert_eq!(ctx.reg.labels[2].runs[0].text, "Multi");
        assert_eq!(ctx.warnings, vec!["line 8: shape `curv-trap` is not supported and `D` is drawn as a rectangle".to_string()]);
    }

    #[test]
    fn arrow_forms() {
        let m = model("flowchart LR\nA --> B\nA --- C\nA -.-> D\nA -.- E\nA ==> F\nA === G\nA ~~~ H\nA --o I\nA --x J\nA <--> K\nA o--o L\nA x--x M");
        let a = |i: usize| &m.links[i].arrow;
        assert_eq!((a(0).head, a(0).tail, a(0).dashed, a(0).bold), (Deco::Point, Deco::None, false, false));
        assert_eq!(a(1).head, Deco::None);
        assert!(a(2).dashed && a(2).head == Deco::Point);
        assert!(a(3).dashed && a(3).head == Deco::None);
        assert!(a(4).bold && a(4).width > a(0).width);
        assert!(a(5).bold && a(5).head == Deco::None);
        assert!(a(6).hidden);
        assert_eq!(a(7).head, Deco::Circle);
        assert_eq!(a(8).head, Deco::Cross);
        assert_eq!((a(9).tail, a(9).head), (Deco::Point, Deco::Point));
        assert_eq!((a(10).tail, a(10).head), (Deco::Circle, Deco::Circle));
        assert_eq!((a(11).tail, a(11).head), (Deco::Cross, Deco::Cross));
        assert!(m.links.iter().all(|l| l.arrow.len >= 2));
    }

    #[test]
    fn longer_arrows_span_more_ranks() {
        let m = model("flowchart TD\nA --> B\nA ---> C\nA ----> D\nA --- E\nA ---- F\nA -..-> G\nA -.-> H\nA ===> I\nA ==> J\nA ~~~~ K");
        let lens: Vec<usize> = m.links.iter().map(|l| l.arrow.len).collect();
        assert_eq!(lens, vec![2, 3, 4, 2, 3, 3, 2, 3, 2, 3]);
    }

    #[test]
    fn circle_and_cross_edges_without_spaces() {
        let m = model("flowchart LR\nA---oB\nC---xD\nE--xF");
        assert_eq!(keys(&m), ["A", "B", "C", "D", "E", "F"]);
        assert_eq!(m.links[0].arrow.head, Deco::Circle);
        assert_eq!(m.links[1].arrow.head, Deco::Cross);
        assert_eq!(m.links[2].arrow.head, Deco::Cross);
    }

    #[test]
    fn dashes_inside_ids() {
        let m = model("flowchart LR\nmy-node --> other-node\nA-->B-C");
        assert_eq!(keys(&m), ["my-node", "other-node", "A", "B-C"]);
    }

    #[test]
    fn edge_labels() {
        let (m, ctx) = parsed("flowchart TD\nA -->|pipe| B\nA -- text --> C\nA -. dotted .-> D\nA == thick ==> E\nA -- open --- F\nA -->|\"q | r\"| G\nA --> H");
        let text = |l: &crate::plantuml::diagrams::gcommon::Link| l.label.map(|id| ctx.reg.labels[id.0].runs[0].text.clone());
        let got: Vec<Option<String>> = m.links.iter().map(text).collect();
        let want = ["pipe", "text", "dotted", "thick", "open", "q | r"].map(|s| Some(s.to_string()));
        assert_eq!(got[..6], want);
        assert_eq!(got[6], None);
        assert!(m.links[2].arrow.dashed && m.links[2].arrow.head == Deco::Point);
        assert!(m.links[3].arrow.bold);
        assert_eq!(m.links[4].arrow.head, Deco::None);
    }

    #[test]
    fn chains_and_fan_out_make_every_pairing_in_order() {
        let m = model("flowchart LR\nA --> B --> C\nD & E --> F & G");
        let pairs: Vec<(&str, &str)> = m.links.iter().map(|l| (m.ents[l.from].key.as_str(), m.ents[l.to].key.as_str())).collect();
        assert_eq!(pairs, [("A", "B"), ("B", "C"), ("D", "F"), ("D", "G"), ("E", "F"), ("E", "G")]);
    }

    #[test]
    fn shapes_inside_chains() {
        let m = model("flowchart LR\nA[One] --> B((Two)):::x --> C{Three}");
        assert_eq!(shape_of(&m, "B"), Shape::Circle);
        assert_eq!(shape_of(&m, "C"), Shape::Rhombus);
        assert_eq!(m.links.len(), 2);
    }

    #[test]
    fn redefinition_keeps_the_last_shape() {
        let m = model("flowchart LR\nA --> B\nA{Pick}\nB");
        assert_eq!(shape_of(&m, "A"), Shape::Rhombus);
        assert_eq!(shape_of(&m, "B"), Shape::Rect);
    }

    #[test]
    fn nodes_belong_to_the_innermost_subgraph_that_closes_first() {
        let m = model("flowchart TD\nsubgraph outer\n X\n subgraph inner\n  X\n  Y\n end\n Z\nend\nsubgraph other\n Z\n W\nend");
        let parent = |k: &str| m.ents[idx(&m, k)].parent.map(|p| m.ents[p].key.as_str());
        assert_eq!(parent("X"), Some("inner"));
        assert_eq!(parent("Y"), Some("inner"));
        assert_eq!(parent("inner"), Some("outer"));
        assert_eq!(parent("Z"), Some("outer"));
        assert_eq!(parent("W"), Some("other"));
        assert_eq!(parent("outer"), None);
    }

    #[test]
    fn a_top_level_mention_moves_into_a_later_subgraph() {
        let m = model("flowchart TD\nA --> B\nsubgraph s\n B\nend");
        let b = idx(&m, "B");
        let s = idx(&m, "s");
        assert_eq!(m.ents[b].parent, Some(s));
        assert_eq!(m.ents[idx(&m, "A")].parent, None);
        // The cluster comes after its member, which the layout has to cope with.
        assert!(s > b);
    }

    #[test]
    fn subgraph_title_forms() {
        let (m, ctx) = parsed("flowchart TD\nsubgraph a [Title A]\nend\nsubgraph b[\"Title B\"]\nend\nsubgraph Spaced title\nend\nsubgraph c\nend\nsubgraph d [ ]\nend");
        assert_eq!(keys(&m), ["a", "b", "Spaced title", "c", "d"]);
        let titles: Vec<String> = ctx.reg.labels.iter().map(|l| l.runs[0].text.clone()).collect();
        assert_eq!(titles, ["Title A", "Title B", "Spaced title", "c"]);
        let Def::Subgraph(s) = &m.ents[4].def else { panic!() };
        assert!(s.title.is_none());
    }

    #[test]
    fn edges_to_subgraphs_end_at_the_cluster() {
        let m = model("flowchart LR\nX --> sg\nsubgraph sg\n A\nend\nsg --> Y");
        assert!(matches!(m.ents[idx(&m, "sg")].def, Def::Subgraph(_)));
        assert_eq!(m.links[0].to, idx(&m, "sg"));
        assert_eq!(m.links[1].from, idx(&m, "sg"));
        assert_eq!(m.ents.len(), 4);
    }

    #[test]
    fn a_subgraph_is_never_its_own_ancestor() {
        let m = model("flowchart TD\nsubgraph A\n subgraph B\n  B --> A\n end\nend");
        assert_eq!(m.ents[idx(&m, "B")].parent, Some(idx(&m, "A")));
        assert_eq!(m.ents[idx(&m, "A")].parent, None);
    }

    #[test]
    fn direction_inside_a_subgraph_warns() {
        let (_, ctx) = parsed("flowchart TD\nsubgraph s\n direction LR\n A\nend");
        assert_eq!(ctx.warnings.len(), 1);
        assert!(ctx.warnings[0].starts_with("line 3: `direction` is ignored"));
    }

    #[test]
    fn styles_resolve_after_collecting() {
        let (m, _) = parsed("flowchart TD\nA:::hot --> B\nstyle B stroke:#00f\nclass B warm\nclassDef warm fill:#ff0\nclassDef hot fill:#f00\nclassDef default stroke-width:2px");
        let css = |k: &str| match &m.ents[idx(&m, k)].def {
            Def::Node(n) => n.css.clone(),
            _ => panic!(),
        };
        assert_eq!(css("A").fill.as_deref(), Some("#ff0000ff"));
        assert_eq!(css("A").stroke_width, Some(2.0));
        assert_eq!(css("B").fill.as_deref(), Some("#ffff00ff"));
        assert_eq!(css("B").stroke.as_deref(), Some("#0000ffff"));
    }

    #[test]
    fn default_class_does_not_style_subgraphs() {
        let (m, _) = parsed("flowchart TD\nclassDef default fill:#f00\nsubgraph s\n A\nend");
        let Def::Subgraph(s) = &m.ents[idx(&m, "s")].def else { panic!() };
        assert_eq!(s.css.fill, None);
    }

    #[test]
    fn link_styles_count_every_pairing_in_source_order() {
        let (m, ctx) = parsed("flowchart TD\nA & B --> C\nC --> D\nlinkStyle default stroke:#111\nlinkStyle 1,2 stroke:#f00,stroke-width:5px,color:#0f0,stroke-dasharray:3 3\nlinkStyle 9 stroke:#000");
        let a = |i: usize| &m.links[i].arrow;
        assert_eq!(a(0).color.as_deref(), Some("#111111ff"));
        assert_eq!(a(1).color.as_deref(), Some("#ff0000ff"));
        assert_eq!(a(2).color.as_deref(), Some("#ff0000ff"));
        assert_eq!((a(1).width, a(1).dashed), (Some(5.0), true));
        assert!(!a(0).dashed);
        assert_eq!(ctx.warnings.len(), 1);
        assert!(ctx.warnings[0].contains("link 9"));
    }

    #[test]
    fn link_label_takes_the_link_text_color() {
        let (m, ctx) = parsed("flowchart TD\nA -->|hi| B\nlinkStyle 0 color:#0f0");
        let id = m.links[0].label.unwrap();
        assert_eq!(ctx.reg.labels[id.0].runs[0].color, "#00ff00ff");
    }

    #[test]
    fn click_is_ignored_with_a_warning() {
        let (m, ctx) = parsed("flowchart TD\nA --> B\nclick A href \"https://x.y\"\ncall cb()\nhref A \"u\"");
        assert_eq!(m.ents.len(), 2);
        assert_eq!(ctx.warnings.len(), 3);
    }

    #[test]
    fn registration_order_is_entities_then_links() {
        let (_, ctx) = parsed("flowchart TD\nA -->|one| B\nC[Third]\nA -->|two| C");
        let t: Vec<&str> = ctx.reg.labels.iter().map(|l| l.runs[0].text.as_str()).collect();
        assert_eq!(t, ["A", "B", "Third", "one", "two"]);
    }

    #[test]
    fn syntax_errors_name_the_line() {
        assert_eq!(fails("flowchart TD\nA --> B\nA[open"), "line 3: unclosed `[`");
        assert_eq!(fails("flowchart TD\nA[\"open] --> B"), "line 2: unclosed quote");
        assert_eq!(fails("flowchart TD\nsubgraph s\n A --> B"), "line 2: subgraph `s` is never closed with `end`");
        assert_eq!(fails("flowchart TD\nA --> B\nend"), "line 3: `end` without a matching `subgraph`");
        assert!(fails("flowchart TD\nA --> end").starts_with("line 2: `end` closes a subgraph"));
        assert!(fails("flowchart TD\nA B").starts_with("line 2: cannot read `B`"));
        assert!(fails("flowchart TD\nA -->").starts_with("line 2: expected a node id"));
        assert!(fails("flowchart TD\nA -- text B").contains("line 2"));
        assert!(fails("flowchart TD\nsubgraph").starts_with("line 2"));
        assert!(fails("flowchart TD\nlinkStyle x stroke:#f00").starts_with("line 2"));
    }

    #[test]
    fn keywords_need_a_second_word_to_be_keywords() {
        let m = model("flowchart TD\nstyle1 --> class2\nendpoint --> End");
        assert_eq!(keys(&m), ["style1", "class2", "endpoint", "End"]);
    }

    #[test]
    fn semicolons_and_empty_diagrams() {
        assert_eq!(model("flowchart TD\nA-->B; B-->C").links.len(), 2);
        assert!(fails("flowchart TD").contains("line 1: Mermaid flowcharts need at least one node"));
    }

    // ---- layout ---------------------------------------------------------------------

    fn laid_out(src: &str) -> (Model, crate::plantuml::layout::gdraw::GraphInfo, Vec<String>) {
        let (m, ctx) = parsed(src);
        let meas = Measured::new(ctx.reg.labels.clone(), sizes(&ctx.reg.labels));
        let (body, info) = layout_info(&m, &ctx.skin, &meas).unwrap();
        (m, info, body.scene.warnings)
    }

    #[test]
    fn typical_fixtures_have_no_layout_warnings() {
        for (name, src) in fixtures("mermaid", "mmd", "flow_") {
            let scene = render_with(labels, mlayout, &src).unwrap();
            let bad: Vec<&String> = scene.warnings.iter().filter(|w| w.starts_with("layout:")).collect();
            // A line over a cluster title is a known limit of dot's cluster handling.
            let bad: Vec<_> = bad.into_iter().filter(|w| !w.contains("crosses the title")).collect();
            assert!(bad.is_empty(), "{name}: {bad:?}");
        }
    }

    fn overlap(a: &crate::plantuml::layout::graph::Rect, b: &crate::plantuml::layout::graph::Rect) -> bool {
        a.x < b.right() - 0.01 && b.x < a.right() - 0.01 && a.y < b.bottom() - 0.01 && b.y < a.bottom() - 0.01
    }

    #[test]
    fn nodes_do_not_overlap_and_clusters_hold_their_members() {
        for (name, src) in fixtures("mermaid", "mmd", "flow_") {
            let (m, info, _) = laid_out(&src);
            for (i, (ea, a)) in info.nodes.iter().enumerate() {
                for (eb, b) in &info.nodes[i + 1..] {
                    assert!(!overlap(a, b), "{name}: {ea} and {eb} overlap");
                }
            }
            for (c, cr) in &info.clusters {
                for (n, nr) in &info.nodes {
                    let mut anc = m.ents[*n].parent;
                    let mut inside = false;
                    while let Some(p) = anc {
                        inside |= p == *c;
                        anc = m.ents[p].parent;
                    }
                    if inside {
                        assert!(nr.x >= cr.x - 0.01 && nr.right() <= cr.right() + 0.01 && nr.y >= cr.y - 0.01 && nr.bottom() <= cr.bottom() + 0.01, "{name}: {n} sticks out of {c}");
                    } else {
                        assert!(!overlap(nr, cr), "{name}: {n} overlaps cluster {c}");
                    }
                }
            }
        }
    }

    #[test]
    fn reversed_directions_flip_the_ranks() {
        let y = |src: &str, key: &str| {
            let (m, info, _) = laid_out(src);
            let i = idx(&m, key);
            info.nodes.iter().find(|(e, _)| *e == i).unwrap().1
        };
        assert!(y("flowchart TD\nA-->B", "A").y < y("flowchart TD\nA-->B", "B").y);
        assert!(y("flowchart BT\nA-->B", "A").y > y("flowchart BT\nA-->B", "B").y);
        assert!(y("flowchart LR\nA-->B", "A").x < y("flowchart LR\nA-->B", "B").x);
        assert!(y("flowchart RL\nA-->B", "A").x > y("flowchart RL\nA-->B", "B").x);
    }

    #[test]
    fn an_empty_subgraph_is_still_drawn_with_its_title() {
        let (m, info, _) = laid_out("flowchart TD\nsubgraph s [Empty one]\nend");
        assert_eq!(info.clusters.len(), 1);
        assert_eq!(info.titles.len(), 1);
        assert!(info.clusters[0].1.w > 0.0 && m.ents.len() == 1);
    }

    #[test]
    fn a_subgraph_declared_after_its_member_lays_out() {
        let (_, info, w) = laid_out("flowchart TD\nA --> B\nsubgraph s\n B\nend");
        assert_eq!((info.nodes.len(), info.clusters.len()), (2, 1));
        assert!(w.is_empty(), "{w:?}");
    }

    #[test]
    fn mangled_sources_never_panic() {
        for (_, src) in fixtures("mermaid", "mmd", "flow_") {
            let lines: Vec<&str> = src.lines().collect();
            for skip in 0..lines.len() {
                let mut l = lines.clone();
                l.remove(skip);
                let _ = render_with(labels, mlayout, &l.join("\n"));
            }
            for (i, line) in lines.iter().enumerate() {
                for at in 0..line.len() {
                    if line.is_char_boundary(at) {
                        let mut l = lines.clone();
                        let cut = line[..at].to_string();
                        l[i] = &cut;
                        let _ = render_with(labels, mlayout, &l.join("\n"));
                    }
                }
            }
        }
    }
}
