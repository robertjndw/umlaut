// Shared page setup for the example pictures in docs/. Diagrams are loaded with
// read(), so the paths below are relative to this folder.
#import "../src/lib.typ": drawio, plantuml

#let diagram(name) = read("diagrams/" + name)

// A page that shrinks to its content, so the PNG has no empty margins to crop.
#let setup(body) = {
  set page(width: auto, height: auto, margin: 8mm)
  set text(font: "New Computer Modern", size: 10pt)
  body
}

// Content with a small caption underneath, the way the README pictures label options.
#let captioned(label, body) = stack(spacing: 3mm, align(center, body), align(center, text(size: 8pt, raw(label))))
