// The picture at the top of the README.
#import "common.typ": *
#show: setup

#grid(
  columns: 2,
  column-gutter: 10mm,
  align: horizon,
  drawio(diagram("architecture.drawio"), width: 11cm),
  plantuml(diagram("login.puml"), width: 7cm),
)
