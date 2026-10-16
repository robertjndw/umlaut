// A diagram without problems compiles even when warnings are errors.
#import "../../src/lib.typ": plantuml
#set page(width: auto, height: auto, margin: 8pt)
#plantuml("class A\nclass B\nA --> B", warn: "error")
