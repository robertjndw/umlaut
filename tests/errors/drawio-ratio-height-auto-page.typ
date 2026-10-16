// expect: container has no fixed height here
#import "../../src/lib.typ": drawio
#set page(width: 10cm, height: auto)
#drawio(read("../../plugin/tests/fixtures/drawio/bare.drawio"), height: 50%)
