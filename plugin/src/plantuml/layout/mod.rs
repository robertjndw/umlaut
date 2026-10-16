//! Layout and drawing. One module per diagram type, plus what they share:
//! `shapes` (arrowheads, notes, icons, translation helpers) and `page` (title, header,
//! footer, caption, legend and background around a laid-out diagram).

pub mod class;
pub mod description;
pub mod flow;
pub mod gdraw;
pub mod graph;
pub mod lint;
pub mod page;
pub mod sequence;
pub mod shapes;
pub mod state;

use crate::scene::Scene;

/// A laid-out diagram without the page furniture: items with the origin at the top
/// left of the diagram's own bounds, and its size. `scene.size` equals `(w, h)`.
pub struct Body {
    pub scene: Scene,
    pub w: f32,
    pub h: f32,
}

impl Body {
    pub fn new(mut scene: Scene, w: f32, h: f32) -> Body {
        scene.size = [w, h];
        Body { scene, w, h }
    }
}
