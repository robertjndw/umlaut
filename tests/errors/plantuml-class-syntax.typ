// expect: line 3: syntax error
#import "../../src/lib.typ": plantuml
#plantuml("@startuml\nclass A\nA <|-- B this is wrong\n@enduml")
