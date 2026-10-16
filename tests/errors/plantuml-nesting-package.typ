// expect: line 202: nesting too deep
#import "../../src/lib.typ": plantuml
#plantuml("@startuml\n" + "package p {\n" * 500 + "class A\n" + "}\n" * 500)
