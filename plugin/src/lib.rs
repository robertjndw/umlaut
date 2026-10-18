//! Core of the umlaut Typst plugin. Everything here is plain Rust so it can be tested
//! natively; the WebAssembly bindings live in `wasm.rs`.
//!
//! Every function returns CBOR that `src/lib.typ` decodes with `cbor()`; the scene
//! layout is documented in `scene.rs`. The front ends (`drawio`, `plantuml`) work on
//! typed values and know nothing about bytes: this file decodes the options and
//! encodes the results.
//!
//! Byte protocol shared with `src/lib.typ`:
//!
//! `drawio(source, opts)` -> scene
//!   source: the .drawio / .drawio.svg file, UTF-8 (or any bytes the decoder can read)
//!   opts:
//!     [0]     page selector: 0 = by index, 1 = by name
//!     [1..5]  u32 LE page index (ignored when selecting by name)
//!     [5..]   UTF-8 page name (empty when selecting by index)
//!
//! `plantuml_labels(source)` -> CBOR list of labels `(runs: (run, ..), wrap_width: f32|none)`
//!   source: the PlantUML text, UTF-8
//!
//! `plantuml_layout(source, sizes)` -> scene
//!   sizes: one pair of f32 LE (width, height) in pt per label, in the order
//!          `plantuml_labels` returned them, as measured by Typst
//!
//! `mermaid_labels(source)` -> CBOR list of labels, like `plantuml_labels`
//!   source: the Mermaid text, UTF-8
//!
//! `mermaid_layout(source, sizes)` -> scene, like `plantuml_layout`
//!
//! `debug_scene()` -> the fixture scene from `scene.rs` (wasm only, for renderer tests)
//!
//! Errors are returned as plain messages and show up as Typst errors, so they should
//! read well to a document author (for PlantUML and Mermaid include the line number).

pub mod color;
pub mod drawio;
pub mod graphviz;
pub mod mermaid;
pub mod plantuml;
pub mod scene;

#[cfg(test)]
pub(crate) mod test_util;

#[cfg(target_arch = "wasm32")]
mod wasm;

/// Which page of a multi-page .drawio file to draw.
#[derive(Debug, Clone, PartialEq)]
pub enum Page {
    Index(usize),
    Name(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct DrawioOptions {
    pub page: Page,
}

fn malformed(what: &str) -> String {
    format!("malformed {what} passed to the umlaut plugin")
}

fn read_f32(bytes: &[u8], at: usize) -> f32 {
    f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn parse_drawio_options(opts: &[u8]) -> Result<DrawioOptions, String> {
    if opts.len() < 5 {
        return Err(malformed("options"));
    }
    let page = match opts[0] {
        0 => Page::Index(u32::from_le_bytes(opts[1..5].try_into().unwrap()) as usize),
        1 => Page::Name(String::from_utf8(opts[5..].to_vec()).map_err(|_| malformed("page name"))?),
        m => return Err(format!("unknown page selector {m}")),
    };
    Ok(DrawioOptions { page })
}

/// A measured length is never negative or non-finite; layout code clamps and divides by
/// these, so anything else becomes an empty label.
fn measured(v: f32) -> f32 {
    if v.is_finite() && v >= 0.0 {
        v
    } else {
        0.0
    }
}

fn parse_sizes(sizes: &[u8]) -> Result<Vec<(f32, f32)>, String> {
    // `chunks` rather than `chunks_exact`/`is_multiple_of`: those lints suggest APIs newer
    // than the oldest Rust this crate builds with.
    sizes
        .chunks(8)
        .map(|c| if c.len() == 8 { Ok((measured(read_f32(c, 0)), measured(read_f32(c, 4)))) } else { Err(malformed("label sizes")) })
        .collect()
}

fn to_cbor<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    ciborium::into_writer(value, &mut out).map_err(|e| format!("could not encode the result: {e}"))?;
    Ok(out)
}

pub fn drawio(source: &[u8], opts: &[u8]) -> Result<Vec<u8>, String> {
    drawio::render(source, &parse_drawio_options(opts)?)?.to_cbor()
}

pub fn plantuml_labels(source: &[u8]) -> Result<Vec<u8>, String> {
    to_cbor(&plantuml::labels(source)?)
}

pub fn plantuml_layout(source: &[u8], sizes: &[u8]) -> Result<Vec<u8>, String> {
    plantuml::layout(source, &parse_sizes(sizes)?)?.to_cbor()
}

pub fn mermaid_labels(source: &[u8]) -> Result<Vec<u8>, String> {
    to_cbor(&mermaid::labels(source)?)
}

pub fn mermaid_layout(source: &[u8], sizes: &[u8]) -> Result<Vec<u8>, String> {
    mermaid::layout(source, &parse_sizes(sizes)?)?.to_cbor()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drawio_options_by_index_and_name() {
        let mut by_index = vec![0];
        by_index.extend(3u32.to_le_bytes());
        assert_eq!(parse_drawio_options(&by_index).unwrap().page, Page::Index(3));

        let mut by_name = vec![1];
        by_name.extend(0u32.to_le_bytes());
        by_name.extend("Page-2".as_bytes());
        assert_eq!(parse_drawio_options(&by_name).unwrap().page, Page::Name("Page-2".into()));
        assert!(parse_drawio_options(&[0]).is_err());
    }

    #[test]
    fn plantuml_sizes() {
        let mut sizes = Vec::new();
        sizes.extend(10f32.to_le_bytes());
        sizes.extend(20f32.to_le_bytes());
        assert_eq!(parse_sizes(&sizes).unwrap(), vec![(10.0, 20.0)]);
        assert!(parse_sizes(&sizes[..7]).is_err());

        let mut odd = Vec::new();
        for v in [f32::NAN, -3.0, f32::INFINITY, 5.0] {
            odd.extend(v.to_le_bytes());
        }
        assert_eq!(parse_sizes(&odd).unwrap(), vec![(0.0, 0.0), (0.0, 5.0)]);
    }
}
