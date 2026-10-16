//! What every diagram type shares while it is parsed and laid out: the label registry,
//! the measured sizes, the page-level texts (title, legend, ...) and the context that
//! carries them.
//!
//! The registry is what makes the two passes line up. While a diagram is parsed, each
//! text it will draw is registered exactly once and gets a `LabelId`, the position in
//! `Registry::labels`. `plantuml::labels` returns that list, Typst measures it, and
//! `plantuml::layout` parses the same source again, which registers the same labels in
//! the same order, so `LabelId` indexes the measured sizes. Because of that, parsing
//! must never branch on anything but the source text, and every `reg.add` has to be
//! reached in the same order on both passes.

use super::layout::shapes::anchor_left;
use super::skin::Skin;
use crate::scene::{HAlign, Label, Run, Scene, TextBox, VAlign};

/// Width added to measured natural-width text so float rounding between the
/// measurement and the drawing can never make the last word wrap.
pub const EPS: f32 = 0.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LabelId(pub usize);

#[derive(Debug, Default)]
pub struct Registry {
    pub labels: Vec<Label>,
}

impl Registry {
    pub fn add(&mut self, runs: Vec<Run>, wrap_width: Option<f32>) -> LabelId {
        self.labels.push(Label { runs, wrap_width });
        LabelId(self.labels.len() - 1)
    }

    /// Registers text that is only drawn when it is not empty.
    pub fn add_opt(&mut self, runs: Vec<Run>, wrap_width: Option<f32>) -> Option<LabelId> {
        if is_blank(&runs) {
            None
        } else {
            Some(self.add(runs, wrap_width))
        }
    }
}

/// True for text with nothing visible to draw: only whitespace and no line breaks.
pub fn is_blank(runs: &[Run]) -> bool {
    runs.iter().all(|r| !r.br && r.text.trim().is_empty())
}

/// The registered labels with the sizes Typst measured for them.
pub struct Measured {
    labels: Vec<Label>,
    sizes: Vec<(f32, f32)>,
}

impl Measured {
    pub fn new(labels: Vec<Label>, sizes: Vec<(f32, f32)>) -> Measured {
        debug_assert_eq!(labels.len(), sizes.len());
        Measured { labels, sizes }
    }

    pub fn size(&self, id: LabelId) -> (f32, f32) {
        self.sizes[id.0]
    }

    pub fn w(&self, id: LabelId) -> f32 {
        self.sizes[id.0].0
    }

    pub fn h(&self, id: LabelId) -> f32 {
        self.sizes[id.0].1
    }

    /// The label's text without markup, line breaks as spaces.
    pub fn plain(&self, id: LabelId) -> String {
        let parts: Vec<&str> = self.labels[id.0].runs.iter().map(|r| if r.br { " " } else { r.text.as_str() }).collect();
        parts.concat().split_whitespace().collect::<Vec<_>>().join(" ")
    }

    pub fn label(&self, id: LabelId) -> &Label {
        &self.labels[id.0]
    }

    /// Text box for the label in the given rectangle. Labels that wrap keep wrapping at
    /// the width they were measured with; natural-width labels never wrap.
    pub fn textbox(&self, id: LabelId, x: f32, y: f32, w: f32, h: f32, ha: HAlign, va: VAlign) -> TextBox {
        let l = &self.labels[id.0];
        TextBox::new(x, y, w, h, l.runs.clone()).align(ha, va).wrap(l.wrap_width.is_some())
    }

    /// Draws the label inside a rectangle.
    pub fn text_in(&self, scene: &mut Scene, id: LabelId, x: f32, y: f32, w: f32, h: f32, ha: HAlign, va: VAlign) {
        scene.push_text(self.textbox(id, x, y, w, h, ha, va));
    }

    /// Draws the label at its measured size. `x` is the left edge, the center or the
    /// right edge depending on `anchor`; `y` is the top.
    pub fn text_at(&self, scene: &mut Scene, id: LabelId, x: f32, y: f32, anchor: HAlign) {
        let (w, h) = self.size(id);
        let w = if self.labels[id.0].wrap_width.is_some() { w } else { w + EPS };
        let left = anchor_left(anchor, x, w);
        scene.push_text(self.textbox(id, left, y, w, h, HAlign::Left, VAlign::Top));
    }
}

/// Where a page-level text goes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub h: HAlign,
    pub top: bool,
}

/// Title, header, footer, caption and legend of the whole diagram.
#[derive(Debug, Clone, Default)]
pub struct Meta {
    pub title: Option<LabelId>,
    pub header: Option<(LabelId, HAlign)>,
    pub footer: Option<(LabelId, HAlign)>,
    pub caption: Option<LabelId>,
    pub legend: Option<(LabelId, Placement)>,
}

/// Everything a diagram parser needs and fills in.
#[derive(Debug)]
pub struct Ctx {
    pub skin: Skin,
    pub reg: Registry,
    pub meta: Meta,
    pub warnings: Vec<String>,
}

impl Ctx {
    pub fn warn(&mut self, line: usize, message: &str) {
        self.warnings.push(format!("line {line}: {message}"));
    }
}
