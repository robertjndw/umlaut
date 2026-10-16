#import "../../src/lib.typ": drawio
#set page(width: auto, height: auto, margin: 8pt)
#let shapes = read("../../plugin/tests/fixtures/drawio/shapes.drawio")
#drawio(shapes)
// Diagrams sized by a ratio are drawn inside `layout`; their warnings must still be found.
#block(width: 200pt, drawio(shapes, width: 100%))
#context {
  let found = query(<umlaut-warnings>)
  assert(found.len() == 2, message: "expected two <umlaut-warnings> elements, got " + str(found.len()))
  let all = found.map(m => m.value).flatten().join("\n")
  assert(all.contains("mxgraph.aws4.lambda"), message: "unknown stencil not reported: " + all)
  assert(all.contains("sketch"), message: "sketch style not reported: " + all)
}
