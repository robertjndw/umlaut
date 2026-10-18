// expect: line 1: expected a diagram type such as `flowchart TD`, found `hello`
#import "../../src/lib.typ": mermaid
#mermaid("hello\nA-->B")
