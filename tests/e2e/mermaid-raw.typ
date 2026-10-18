#import "../../src/lib.typ": mermaid
#set page(width: auto, height: auto, margin: 8pt)
#show raw.where(lang: "mermaid"): it => mermaid(it.text)

```mermaid
flowchart LR
    A[Write] --> B{Review}
    B -->|ok| C[Ship]
    B -->|changes| A
```
