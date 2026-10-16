// expect: line 3: syntax error
#import "../../src/lib.typ": plantuml
#plantuml("@startuml\nAlice -> Bob : hi\nthis is not valid\n@enduml")
