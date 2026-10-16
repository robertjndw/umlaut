// expect: line 3: this `if` is never closed
#import "../../src/lib.typ": plantuml
#plantuml("start\n:a;\nif (x?) then (yes)\n:b;\nstop")
