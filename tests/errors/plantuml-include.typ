// expect: line 2: the preprocessor directive !include is not supported
#import "../../src/lib.typ": plantuml
#plantuml("A -> B\n!include other.puml")
