// The "Styling" section of the README: font, size, scale and inline PlantUML blocks.
#import "common.typ": *
#show: setup

// The raw block is in the monospace font, so the diagram would be too; pass the font you want.
#show raw.where(lang: "plantuml"): it => plantuml(it.text, font: "New Computer Modern")

#let src = diagram("login.puml")
#grid(
  columns: 3,
  column-gutter: 8mm,
  row-gutter: 6mm,
  align: top,
  captioned("font: \"DejaVu Sans Mono\"", plantuml(src, font: "DejaVu Sans Mono", width: 5cm)),
  captioned("width: 5cm", plantuml(src, width: 5cm)),
  captioned("scale: 40%", plantuml(src, scale: 40%)),
  grid.cell(colspan: 3, captioned("inline raw block", [
    ```plantuml
    Alice -> Bob : hello
    Bob --> Alice : hi
    ```
  ])),
)
