// expect: line 2: this group is never closed
#import "../../src/lib.typ": plantuml
#plantuml("A -> B\nalt x\nB -> A")
