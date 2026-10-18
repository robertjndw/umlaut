// A diagram without problems compiles even when warnings are errors.
#import "../../src/lib.typ": mermaid
#set page(width: auto, height: auto, margin: 8pt)
#mermaid("flowchart LR\n  A[Start] --> B{Check}\n  B -->|yes| C[Done]\n  B -->|no| A", warn: "error")
