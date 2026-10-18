#import "../../src/lib.typ": mermaid
#set page(width: auto, height: auto, margin: 8pt)
#mermaid("%%{init: {'theme': 'dark'}}%%\nflowchart LR\n  A --> B\n  click A href \"https://example.com\"")
#context {
  let found = query(<umlaut-warnings>)
  assert(found.len() == 1, message: "expected one <umlaut-warnings> element")
  let text = found.first().value.join("\n")
  assert(text.contains("init") and text.contains("click"), message: "warnings missing: " + text)
}
