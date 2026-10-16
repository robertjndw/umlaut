// Page selection for multi-page draw.io files, by index and by name.
#import "common.typ": *
#show: setup

#let src = diagram("pages.drawio")
#grid(
  columns: 3,
  column-gutter: 8mm,
  captioned("page: 0", drawio(src, width: 5cm)),
  captioned("page: 1", drawio(src, page: 1, width: 5cm)),
  captioned("page: \"Details\"", drawio(src, page: "Details", width: 5cm)),
)
