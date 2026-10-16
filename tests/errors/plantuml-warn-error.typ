// expect: the diagram has problems (pass `warn: true` to draw it anyway)
#import "../../src/lib.typ": plantuml
#plantuml("!theme cerulean\nA -> B", warn: "error")
