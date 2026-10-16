// expect: nesting too deep
#import "../../src/lib.typ": plantuml
#plantuml("start\n" + "if (c) then (y)\n" * 500 + ":a;\n" + "endif\n" * 500 + "stop")
