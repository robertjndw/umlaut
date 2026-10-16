// expect: line 3: syntax error
#import "../../src/lib.typ": plantuml
#plantuml("@startuml\n[*] --> A\nA is nonsense\n@enduml")
