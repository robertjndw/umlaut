// expect: page index 9 is out of range: the file has 3 pages
#import "../../src/lib.typ": drawio
#drawio(read("../../plugin/tests/fixtures/drawio/multipage.drawio"), page: 9)
