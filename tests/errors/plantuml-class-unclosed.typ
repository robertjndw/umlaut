// expect: line 2: the body of `A` is never closed
#import "../../src/lib.typ": plantuml
#plantuml("@startuml\nclass A {\n  +x : int\n@enduml")
