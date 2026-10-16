use wasm_minimal_protocol::*;

initiate_protocol!();

#[wasm_func]
pub fn drawio(source: &[u8], opts: &[u8]) -> Result<Vec<u8>, String> {
    crate::drawio(source, opts)
}

#[wasm_func]
pub fn plantuml_labels(source: &[u8]) -> Result<Vec<u8>, String> {
    crate::plantuml_labels(source)
}

#[wasm_func]
pub fn plantuml_layout(source: &[u8], sizes: &[u8]) -> Result<Vec<u8>, String> {
    crate::plantuml_layout(source, sizes)
}

/// Hand-written scene for checking the Typst renderer on its own.
#[wasm_func]
pub fn debug_scene() -> Result<Vec<u8>, String> {
    crate::scene::fixture().to_cbor()
}
