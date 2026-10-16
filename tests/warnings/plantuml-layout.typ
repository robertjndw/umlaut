// The exact problems depend on the measured labels, so this pins the font and only
// checks that layout problems reach the metadata; plugin/src/plantuml/layout/lint.rs
// tests the individual checks.
#import "../../src/lib.typ": plantuml
#set page(width: auto, height: auto, margin: 8pt)
#set text(font: "Libertinus Serif")
#plantuml(read("../../plugin/tests/fixtures/plantuml/cmp_large.puml"))
#context {
  let found = query(<umlaut-warnings>)
  assert(found.len() == 1, message: "expected one <umlaut-warnings> element")
  let layout = found.first().value.filter(w => w.starts-with("layout: "))
  assert(layout.len() > 0, message: "no layout problems reported")
}
