#import "../../src/lib.typ": _debug-scene
#set page(width: auto, height: auto, margin: 8pt)
#_debug-scene(warn: true)
#context {
  let found = query(<umlaut-warnings>)
  assert(found.len() == 1, message: "expected one <umlaut-warnings> element")
  assert(found.first().value.any(w => w.contains("mxgraph.fake")), message: "warning text missing")
}
