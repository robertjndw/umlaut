// expect: line 2: timing diagrams are not supported yet
#import "../../src/lib.typ": plantuml
#plantuml("@startuml\nrobust \"Web\" as W\n@0\nW is Idle\n@enduml")
