#import "../../src/lib.typ": plantuml
#set page(width: auto, height: auto, margin: 8pt)
#plantuml(read("../../plugin/tests/fixtures/plantuml/misc_warnings.puml"))
#context {
  let found = query(<umlaut-warnings>)
  assert(found.len() == 1, message: "expected one <umlaut-warnings> element")
  let text = found.first().value.join("\n")
  assert(text.contains("!theme") and text.contains("handwritten"), message: "warnings missing: " + text)
}
