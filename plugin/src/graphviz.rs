//! Safe wrapper around the Graphviz dot layout compiled in by `build.rs`. The C side
//! (graphviz/shim.c) takes DOT text and returns plain numbers; this module only
//! decodes them. Everything about what to put into the DOT text lives in
//! `plantuml/layout/graph.rs`.

use std::ffi::{c_char, c_int, CStr, CString};
use std::sync::Mutex;

extern "C" {
    fn umlaut_layout(dot: *const c_char, out: *mut *mut f64, len: *mut usize, err: *mut *const c_char) -> c_int;
    fn umlaut_free(p: *mut f64);
}

/// Graphviz keeps its attribute tables and option flags in C globals, so two layouts
/// must never run at once (native tests run on several threads; Typst never does).
static GRAPHVIZ: Mutex<()> = Mutex::new(());

/// A placed node: centre and size in pt, y pointing down.
#[derive(Debug, Clone, Copy)]
pub struct PlacedNode {
    pub id: usize,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// A cluster's outline: left, top, right, bottom in pt.
#[derive(Debug, Clone, Copy)]
pub struct PlacedCluster {
    pub id: usize,
    pub rect: [f64; 4],
}

#[derive(Debug, Clone, Default)]
pub struct PlacedEdge {
    pub id: usize,
    /// Centres of the edge label, the label at the tail and the label at the head.
    pub label: Option<(f64, f64)>,
    pub tail_label: Option<(f64, f64)>,
    pub head_label: Option<(f64, f64)>,
    /// Cubic Bezier segments from the tail to the head.
    pub curves: Vec<Vec<(f64, f64)>>,
}

#[derive(Debug, Clone, Default)]
pub struct Placed {
    pub size: (f64, f64),
    pub nodes: Vec<PlacedNode>,
    pub clusters: Vec<PlacedCluster>,
    pub edges: Vec<PlacedEdge>,
}

struct Reader<'a> {
    v: &'a [f64],
    at: usize,
}

impl Reader<'_> {
    fn next(&mut self) -> f64 {
        let x = self.v.get(self.at).copied().unwrap_or(0.0);
        self.at += 1;
        x
    }

    fn label(&mut self) -> Option<(f64, f64)> {
        let set = self.next() != 0.0;
        let p = (self.next(), self.next());
        set.then_some(p)
    }
}

/// Runs dot on `dot`. The graph's nodes must be named `n<k>`, its clusters
/// `cluster_<k>` and its edges carry `comment="<k>"`; the numbers come back as ids.
pub fn layout(dot: &str) -> Result<Placed, String> {
    let text = CString::new(dot).map_err(|_| "the graph text contains a NUL byte".to_string())?;
    let _guard = GRAPHVIZ.lock().unwrap_or_else(|e| e.into_inner());
    let (mut out, mut len, mut err) = (std::ptr::null_mut(), 0usize, std::ptr::null());
    // SAFETY: the shim only reads `text` and writes the three out-parameters; on success
    // `out` is a heap block of `len` doubles that is freed below.
    let rc = unsafe { umlaut_layout(text.as_ptr(), &mut out, &mut len, &mut err) };
    if rc != 0 {
        // SAFETY: on failure `err` points at the shim's static message buffer.
        let msg = unsafe { CStr::from_ptr(err) }.to_string_lossy().trim().to_string();
        return Err(format!("graph layout failed: {msg}"));
    }
    // SAFETY: see above; the slice is copied out before the block is freed.
    let numbers = unsafe { std::slice::from_raw_parts(out, len) }.to_vec();
    unsafe { umlaut_free(out) };

    let mut r = Reader { v: &numbers, at: 0 };
    let (x0, y0, x1, y1) = (r.next(), r.next(), r.next(), r.next());
    let (nn, nc, ne) = (r.next() as usize, r.next() as usize, r.next() as usize);
    let mut placed = Placed { size: (x1 - x0, y1 - y0), ..Placed::default() };
    for _ in 0..nn {
        let id = r.next() as usize;
        placed.nodes.push(PlacedNode { id, x: r.next() - x0, y: r.next() - y0, w: r.next(), h: r.next() });
    }
    for _ in 0..nc {
        let id = r.next() as usize;
        let (a, b, c, d) = (r.next(), r.next(), r.next(), r.next());
        placed.clusters.push(PlacedCluster { id, rect: [a - x0, b - y0, c - x0, d - y0] });
    }
    let shift = |p: Option<(f64, f64)>| p.map(|(x, y)| (x - x0, y - y0));
    for _ in 0..ne {
        let id = r.next() as usize;
        let (label, tail_label, head_label) = (shift(r.label()), shift(r.label()), shift(r.label()));
        let curves = (0..r.next() as usize)
            .map(|_| {
                let n = r.next() as usize;
                (0..n).map(|_| (r.next() - x0, r.next() - y0)).collect()
            })
            .collect();
        placed.edges.push(PlacedEdge { id, label, tail_label, head_label, curves });
    }
    if r.at > numbers.len() {
        return Err("graph layout returned a truncated result".into());
    }
    Ok(placed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_places_a_chain_top_down() {
        let p = layout("digraph { node [shape=box fixedsize=true width=1 height=0.5 label=\"\"]; n0; n1; n0 -> n1 [comment=\"0\"]; }").unwrap();
        assert_eq!(p.nodes.len(), 2);
        let (a, b) = (p.nodes.iter().find(|n| n.id == 0).unwrap(), p.nodes.iter().find(|n| n.id == 1).unwrap());
        assert!(b.y > a.y, "the head sits below the tail: {a:?} {b:?}");
        assert_eq!(p.edges[0].curves.len(), 1);
    }

    #[test]
    fn bad_input_is_an_error_not_a_crash() {
        assert!(layout("digraph { a -> ").is_err());
    }
}

