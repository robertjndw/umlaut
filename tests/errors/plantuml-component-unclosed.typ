// expect: line 2: this block is never closed
#import "../../src/lib.typ": plantuml
#plantuml("@startuml\nnode Server {\n  [App]\n@enduml")
