//! Mermaid source to scene. There's no Mermaid model or layout of its own (except for
//! flowcharts): each parser fills the model the matching PlantUML parser would, and the
//! PlantUML layout draws it with Mermaid's theme. The lexer hands out PlantUML `Line`s
//! so `Cursor`, `err` and `check_nesting` work as they are.
//!
//! Each diagram module has a `parse(lines, header, ctx)` and a `layout(model, skin,
//! meas)`. `ctx` comes in with the Mermaid skin and the front matter title already
//! registered. Parsers only touch `ctx.meta` through `set_title`.
//!
//! Parsing is two-phase because `classDef`, `style`, `linkStyle` and `class` can come
//! after the thing they style. Phase one only collects strings and styles. Phase two
//! registers labels: title, then entities in creation order, then links in source
//! order. Pass 2 matches sizes to labels by index, so that order has to depend on the
//! source and nothing else.

pub mod class;
pub mod css;
pub mod flowchart;
pub mod lexer;
pub mod sequence;
pub mod state;
pub mod text;
pub mod theme;

use crate::plantuml::layout::page;
use crate::plantuml::model::{Ctx, Measured, Registry};
use crate::scene::{Label, Scene};
use lexer::Kind;

enum Diagram {
    Flowchart(flowchart::Model),
    Sequence(sequence::Model),
    Class(class::Model),
    State(state::Model),
}

struct Parsed {
    ctx: Ctx,
    diagram: Diagram,
}

/// Registers the diagram title (a `title` statement a parser found) unless the front
/// matter already set one. Call it first in the registration phase.
pub fn set_title(ctx: &mut Ctx, title: &str) {
    if ctx.meta.title.is_none() {
        let runs = text::runs(title, &theme::title(&ctx.skin));
        ctx.meta.title = ctx.reg.add_opt(runs, None);
    }
}

struct Front {
    header: lexer::Header,
    lines: Vec<crate::plantuml::lexer::Line>,
    ctx: Ctx,
}

fn front(source: &[u8]) -> Result<Front, String> {
    let lexer::Source { header, lines, title, wrap, warnings } = lexer::lex(source)?;
    let mut ctx = Ctx { skin: theme::skin(), reg: Registry::default(), meta: Default::default(), warnings };
    // The init directive itself is ignored, but `wrap: true` is how sequence diagrams
    // ask for wrapped messages; parsers read it back as `ctx.skin.flag("wrap")`.
    if wrap {
        ctx.skin.set("wrap", "true");
    }
    if let Some(t) = title {
        set_title(&mut ctx, &t);
    }
    Ok(Front { header, lines, ctx })
}

fn parse(source: &[u8]) -> Result<Parsed, String> {
    let Front { header, lines, mut ctx } = front(source)?;
    let diagram = match header.kind {
        Kind::Flowchart => Diagram::Flowchart(flowchart::parse(&lines, &header, &mut ctx)?),
        Kind::Sequence => Diagram::Sequence(sequence::parse(&lines, &header, &mut ctx)?),
        Kind::Class => Diagram::Class(class::parse(&lines, &header, &mut ctx)?),
        Kind::State => Diagram::State(state::parse(&lines, &header, &mut ctx)?),
    };
    Ok(Parsed { ctx, diagram })
}

fn check_sizes(expected: usize, got: usize) -> Result<(), String> {
    if expected != got {
        return Err(format!("the plugin expected {expected} label sizes but got {got}; the source changed between the two passes"));
    }
    Ok(())
}

/// Pass 1: parse and return every label whose size the layout needs, in a stable order.
/// `layout` receives one measured size per label, in this order, so the order must be
/// a pure function of `source`.
pub fn labels(source: &[u8]) -> Result<Vec<Label>, String> {
    Ok(parse(source)?.ctx.reg.labels)
}

/// Pass 2: lay out with `sizes[i] = (width, height)` in pt for `labels(source)[i]`.
pub fn layout(source: &[u8], sizes: &[(f32, f32)]) -> Result<Scene, String> {
    let Parsed { ctx, diagram } = parse(source)?;
    check_sizes(ctx.reg.labels.len(), sizes.len())?;
    let Ctx { skin, reg, meta, warnings } = ctx;
    let measured = Measured::new(reg.labels, sizes.to_vec());
    let body = match &diagram {
        Diagram::Flowchart(m) => flowchart::layout(m, &skin, &measured)?,
        Diagram::Sequence(m) => sequence::layout(m, &skin, &measured)?,
        Diagram::Class(m) => class::layout(m, &skin, &measured)?,
        Diagram::State(m) => state::layout(m, &skin, &measured)?,
    };
    let mut scene = page::compose(body, &meta, &skin, &measured);
    for w in warnings {
        scene.warn(w);
    }
    Ok(scene)
}

#[cfg(test)]
mod tests;
