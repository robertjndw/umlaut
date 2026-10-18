//! Mermaid's default theme, expressed as the skinparams and shape choices the PlantUML
//! layouts already read. Values come from `themes/theme-default.js` in the Mermaid
//! repository; the variable each one comes from is named next to it.
//!
//! Units: Mermaid sizes are CSS px and the scene is in pt, but PlantUML's px are drawn
//! 1:1 as pt too, so Mermaid's numbers are used as they are (16 px text is 16 here).
//! That keeps Mermaid and PlantUML pictures at a comparable size on the page, and
//! `width`/`scale` resize either one. No font name is set: with `font: auto` the
//! document font is used, not Mermaid's "trebuchet ms".

use crate::plantuml::creole::Base;
use crate::plantuml::layout::shapes::NoteKind;
use crate::plantuml::skin::Skin;
use crate::scene::Paint;

/// `primaryColor` / `mainBkg` / `actorBkg` / `stateBkg`.
pub const PRIMARY: &str = "#ECECFF";
/// `border1`, which is `nodeBorder`, `primaryBorderColor` and `actorBorder`.
pub const BORDER: &str = "#9370DB";
/// `lineColor`, `signalColor` and `arrowheadColor`.
pub const LINE: &str = "#333333";
/// `textColor`.
pub const TEXT: &str = "#333333";
/// `primaryTextColor` (`invert(primaryColor)`), used for class and state text.
pub const PRIMARY_TEXT: &str = "#131300";
/// `actorTextColor`, `noteTextColor`, `labelTextColor`.
pub const ACTOR_TEXT: &str = "#000000";
/// `noteBkgColor`.
pub const NOTE_FILL: &str = "#fff5ad";
/// `border2`: `noteBorderColor` and `clusterBorder`.
pub const BORDER2: &str = "#aaaa33";
/// `clusterBkg` (`secondBkg`).
pub const CLUSTER_FILL: &str = "#ffffde";
/// `activationBkgColor`.
pub const ACTIVATION_FILL: &str = "#f4f4f4";
/// `activationBorderColor`.
pub const ACTIVATION_BORDER: &str = "#666666";
/// `edgeLabelBackground` is rgba(232,232,232, 0.8).
const EDGE_LABEL_BACKGROUND: &str = "#e8e8e8cc";
/// `fontSize`.
pub const FONT_SIZE: f32 = 16.0;
/// Page title size; Mermaid draws the front matter title larger than the body text.
pub const TITLE_SIZE: f32 = 18.0;
/// `flowchart.nodeSpacing` and `flowchart.rankSpacing`.
const SPACING: &str = "50";

pub fn skin() -> Skin {
    let mut s = Skin::new();
    let mut set = |k: &str, v: &str| {
        s.set(k, v);
    };
    set("defaultFontSize", "16");
    set("defaultFontColor", TEXT);
    set("titleFontSize", "18");
    set("nodesep", SPACING);
    set("ranksep", SPACING);

    // Edges and arrows (lineColor, signalColor).
    set("arrowColor", LINE);
    set("arrowFontColor", TEXT);
    set("sequenceArrowThickness", "1.5");

    // Flowchart nodes, read by the flowchart layout through these keys.
    set("nodeBackgroundColor", PRIMARY);
    set("nodeBorderColor", BORDER);
    set("nodeFontColor", TEXT);
    set("nodeRoundCorner", "10");

    // Sequence diagrams: actorBkg, actorBorder, actorTextColor, actorLineColor.
    set("participantBackgroundColor", PRIMARY);
    set("participantBorderColor", BORDER);
    set("participantFontColor", ACTOR_TEXT);
    set("participantRoundCorner", "6");
    set("sequenceLifeLineBorderColor", BORDER);
    set("sequenceMessageFontColor", TEXT);
    // Activation boxes differ from the lifeline, hence their own keys.
    set("sequenceActivationBackgroundColor", ACTIVATION_FILL);
    set("sequenceActivationBorderColor", ACTIVATION_BORDER);
    // Loop frames: labelBoxBkgColor, labelBoxBorderColor, labelTextColor.
    set("sequenceGroupBorderColor", BORDER);
    set("sequenceGroupHeaderBackgroundColor", PRIMARY);
    set("sequenceGroupFontColor", ACTOR_TEXT);

    // Notes: noteBkgColor, noteBorderColor, noteTextColor.
    set("noteBackgroundColor", NOTE_FILL);
    set("noteBorderColor", BORDER2);
    set("noteFontColor", ACTOR_TEXT);

    // Subgraphs, namespaces and composite states: clusterBkg, clusterBorder.
    set("packageBackgroundColor", CLUSTER_FILL);
    set("packageBorderColor", BORDER2);
    set("packageFontColor", TEXT);

    // Classes and states: mainBkg, nodeBorder, classText.
    set("classBackgroundColor", PRIMARY);
    set("classBorderColor", BORDER);
    set("classFontColor", PRIMARY_TEXT);
    set("classArrowColor", LINE);
    set("stateBackgroundColor", PRIMARY);
    set("stateBorderColor", BORDER);
    set("stateFontColor", PRIMARY_TEXT);
    set("stateRoundCorner", "10");
    set("stateArrowColor", LINE);
    // specialStateColor, the start dot and the end ring.
    set("stateStartColor", LINE);

    let st = &mut s.style;
    // Mermaid draws solid, thin lifelines and a flat rectangle for notes.
    st.lifeline_dash = None;
    st.lifeline_width = 0.5;
    st.participant_width = 1.0;
    st.box_border = crate::scene::hex(BORDER);
    st.box_width = 1.0;
    // Loop frames are dashed in the default theme (`loopLine`).
    st.group_dash = Some([2.0, 2.0]);
    st.group_width = 1.0;
    st.note_kind = NoteKind::Rect;
    // `.edge-pattern-dashed` uses stroke-dasharray 3.
    st.link_dash = [3.0, 3.0];
    st.label_background = Some(Paint::solid(EDGE_LABEL_BACKGROUND));
    s
}

// Base text styles per role, so every parser builds runs the same way. The keys are the
// ones the PlantUML layouts look the same text up under.

pub fn title(skin: &Skin) -> Base {
    skin.text(&["title"], TITLE_SIZE, TEXT)
}

/// Flowchart node labels.
pub fn node(skin: &Skin) -> Base {
    skin.text(&["node"], FONT_SIZE, TEXT)
}

/// Edge and relation labels.
pub fn edge(skin: &Skin) -> Base {
    skin.text(&["arrow"], FONT_SIZE, TEXT)
}

/// Sequence message text.
pub fn message(skin: &Skin) -> Base {
    skin.text(&["sequencemessage", "sequencearrow", "arrow"], FONT_SIZE, TEXT)
}

/// Participant and actor names; `kind` is the keyword (`participant`, `actor`).
pub fn actor(skin: &Skin, kind: &str) -> Base {
    skin.text(&[&format!("sequence{kind}"), kind, "sequenceparticipant", "participant"], FONT_SIZE, ACTOR_TEXT)
}

pub fn note(skin: &Skin) -> Base {
    skin.text(&["note"], FONT_SIZE, ACTOR_TEXT)
}

/// Subgraph, namespace and composite state titles.
pub fn cluster(skin: &Skin) -> Base {
    skin.text(&["package"], FONT_SIZE, TEXT)
}

pub fn class_name(skin: &Skin) -> Base {
    skin.text(&["class"], FONT_SIZE, PRIMARY_TEXT)
}

pub fn class_member(skin: &Skin) -> Base {
    skin.text(&["classattribute", "class"], FONT_SIZE, PRIMARY_TEXT)
}

pub fn state(skin: &Skin) -> Base {
    skin.text(&["state"], FONT_SIZE, PRIMARY_TEXT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plantuml::skin as ps;

    #[test]
    fn seeds_the_default_theme() {
        let s = skin();
        assert_eq!(s.fill(&["participant"], ps::PARTICIPANT_FILL).color, "#ececffff");
        assert_eq!(s.border(&["participant"], ps::BORDER), "#9370dbff");
        assert_eq!(s.fill(&["note"], ps::NOTE_FILL).color, "#fff5adff");
        assert_eq!(s.border(&["package"], ps::BORDER), "#aaaa33ff");
        assert_eq!(s.num(&["nodesep"], 0.0), 50.0);
        assert_eq!(s.fill(&["sequenceactivation", "sequencelifeline"], "#000000").color, "#f4f4f4ff");
        assert!(s.style.lifeline_dash.is_none());
        assert_eq!(s.style.note_kind, NoteKind::Rect);
    }

    #[test]
    fn roles_share_one_size_and_color() {
        let s = skin();
        assert_eq!(node(&s).size, 16.0);
        assert_eq!(message(&s).color, crate::scene::hex(TEXT));
        assert_eq!(actor(&s, "actor").color, crate::scene::hex(ACTOR_TEXT));
        assert_eq!(title(&s).size, 18.0);
        assert_eq!(class_name(&s).color, crate::scene::hex(PRIMARY_TEXT));
    }
}
