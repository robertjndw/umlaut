// expect: line 1: the front matter opened here with --- is never closed
#import "../../src/lib.typ": mermaid
#mermaid("---\ntitle: Orders\nflowchart TD\nA-->B")
