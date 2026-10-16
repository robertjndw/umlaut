// The "Figures" section of the README: captions, references and a list of diagrams.
#import "common.typ": *
#show: setup
// The outline's dot leaders need a page with a fixed width to run across.
#set page(width: 16cm)

#outline(title: [Diagrams], target: figure.where(kind: "diagram"))

#let diagram-figure = figure.with(kind: "diagram", supplement: [Diagram])

#grid(
  columns: 2,
  column-gutter: 8mm,
  align: bottom,
  [#diagram-figure(
    plantuml(diagram("login.puml"), width: 100%),
    caption: [Logging in],
  ) <login>],
  [#diagram-figure(
    drawio(diagram("pages.drawio"), width: 100%),
    caption: [First page of `pages.drawio`],
  ) <pages>],
)

@login shows the messages exchanged during login, and @pages the first page of a
multi-page draw.io file.
