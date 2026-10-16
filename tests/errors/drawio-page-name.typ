// expect: page "Missing" not found in the draw.io file; available pages: 0: "Overview", 1: "Details", 2: "Page-3"
#import "../../src/lib.typ": drawio
#drawio(read("../../plugin/tests/fixtures/drawio/multipage.drawio"), page: "Missing")
