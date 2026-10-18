//! PlantUML source to scene, in two passes so Typst can measure the text in between.
//!
//! Layout of the module:
//!
//! - `lexer`: comments, `@start`/`@end`, continuation lines, directives, line cursor.
//! - `meta`: title/header/footer/caption/legend/skinparam, shared by all diagrams.
//! - `skin`, `colors`, `creole`: the look, color names and text markup.
//! - `model`: the label registry (`Registry`, `LabelId`), measured sizes (`Measured`)
//!   and the parse context (`Ctx`).
//! - `detect`: which diagram type a source is.
//! - `diagrams/<type>.rs`: parser and model for one type. It receives the remaining
//!   lines and the `Ctx`, registers every text it will draw and returns its model.
//! - `layout/<type>.rs`: turns a model plus `Measured` into a `layout::Body` (a scene
//!   with its size). `layout::shapes` holds the drawing helpers shared by all types
//!   and `layout::page` wraps a body with title, header, footer, caption and legend.
//!
//! Class, state, use case and component diagrams share `diagrams/gcommon` (links,
//! arrows, notes, styles) and `layout/graph` (the layered layout) plus `layout/gdraw`
//! (links, containers, notes); use case and component diagrams are one parser,
//! `diagrams/description`.
//!
//! To add a diagram type: add a `Kind` arm in `detect.rs`, write `diagrams/x.rs` and
//! `layout/x.rs`, extend `Diagram` and the two `match`es below. A graph layout shared
//! by several types belongs in `layout/graph.rs`.

// Drawing helpers naturally take a coordinate or two more than clippy likes.
#![allow(clippy::too_many_arguments)]

pub mod colors;
pub mod creole;
pub mod detect;
pub mod diagrams;
pub mod layout;
pub mod lexer;
pub mod meta;
pub mod model;
pub mod skin;

use crate::scene::{Label, Scene};
use detect::Kind;
use model::{Ctx, Measured, Registry};

enum Diagram {
    Sequence(diagrams::sequence::Model),
    Activity(diagrams::activity::Model),
    Class(diagrams::class::Model),
    State(diagrams::state::Model),
    Description(diagrams::description::Model),
}

struct Parsed {
    ctx: Ctx,
    diagram: Diagram,
}

/// What is left of a source once the statements every diagram shares are handled.
struct Front {
    /// The lines for the diagram's parser.
    lines: Vec<lexer::Line>,
    /// With the meta labels already registered.
    ctx: Ctx,
    tag: String,
    tag_line: usize,
}

fn front(source: &[u8]) -> Result<Front, String> {
    let lexer::Source { tag, tag_line, lines, mut warnings } = lexer::lex(source)?;
    let mut skin = skin::Skin::new();
    let (lines, raw) = meta::extract(lines, &mut skin, &mut warnings)?;
    let mut ctx = Ctx { skin, reg: Registry::default(), meta: Default::default(), warnings };
    meta::register(raw, &mut ctx);
    Ok(Front { lines, ctx, tag, tag_line })
}

/// Runs one diagram parser on `src` without type detection, so tests can feed any
/// snippet to the parser they exercise.
#[cfg(test)]
pub(crate) fn parse_with<M>(src: &str, parse: fn(&[lexer::Line], &mut Ctx) -> Result<M, String>) -> Result<(M, Ctx), String> {
    let Front { lines, mut ctx, .. } = front(src.as_bytes())?;
    let m = parse(&lines, &mut ctx)?;
    Ok((m, ctx))
}

fn parse(source: &[u8]) -> Result<Parsed, String> {
    parse_diagram(source).map_err(|e| mermaid_hint(source, e))
}

fn parse_diagram(source: &[u8]) -> Result<Parsed, String> {
    let Front { lines, mut ctx, tag, tag_line } = front(source)?;
    // `meta::register` cannot fail, so detecting after it reports the same errors.
    let (kind, line) = detect::detect(&tag, tag_line, &lines)?;
    let diagram = match kind {
        Kind::Sequence => Diagram::Sequence(diagrams::sequence::parse(&lines, &mut ctx)?),
        Kind::Activity => Diagram::Activity(diagrams::activity::parse(&lines, &mut ctx)?),
        Kind::Class => Diagram::Class(diagrams::class::parse(&lines, &mut ctx)?),
        Kind::State => Diagram::State(diagrams::state::parse(&lines, &mut ctx)?),
        Kind::UseCase | Kind::Component => Diagram::Description(diagrams::description::parse(&lines, &mut ctx)?),
        Kind::LegacyActivity => return Err(format!("line {line}: legacy activity syntax is not supported yet, use the new syntax (start, :action;, stop)")),
        other => return Err(format!("line {line}: {} diagrams are not supported yet", other.name())),
    };
    Ok(Parsed { ctx, diagram })
}

/// Mermaid sources that were handed to `plantuml()` fail with PlantUML syntax errors that
/// say nothing about the real mistake, so the error gets a pointer.
fn mermaid_hint(source: &[u8], e: String) -> String {
    let first = String::from_utf8_lossy(source);
    let line = first.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let (word, _) = crate::mermaid::lexer::split_word(line);
    if crate::mermaid::lexer::header_kind(word).is_some() {
        format!("{e} (this looks like Mermaid, use mermaid())")
    } else {
        e
    }
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
    finish(ctx, sizes, |skin, measured| {
        Ok(match &diagram {
            Diagram::Sequence(m) => layout::sequence::layout(m, skin, measured),
            Diagram::Activity(m) => layout::flow::layout(m, skin, measured),
            Diagram::Class(m) => layout::class::layout(m, skin, measured)?,
            Diagram::State(m) => layout::state::layout(m, skin, measured)?,
            Diagram::Description(m) => layout::description::layout(m, skin, measured)?,
        })
    })
}

/// The end of pass 2 for every front end that parses into a `Ctx`: checks that each
/// registered label got a size, lays out the body with the measured labels and puts the
/// page (title, legend, ...) and the parser's warnings around it.
pub(crate) fn finish(ctx: Ctx, sizes: &[(f32, f32)], body: impl FnOnce(&skin::Skin, &Measured) -> Result<layout::Body, String>) -> Result<Scene, String> {
    if sizes.len() != ctx.reg.labels.len() {
        return Err(format!(
            "the plugin expected {} label sizes but got {}; the source changed between the two passes",
            ctx.reg.labels.len(),
            sizes.len()
        ));
    }
    let Ctx { skin, reg, meta, warnings } = ctx;
    let measured = Measured::new(reg.labels, sizes.to_vec());
    let mut scene = layout::page::compose(body(&skin, &measured)?, &meta, &skin, &measured);
    for w in warnings {
        scene.warn(w);
    }
    Ok(scene)
}

#[cfg(test)]
mod tests;
