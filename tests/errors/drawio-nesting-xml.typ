// expect: nested too deeply
#import "../../src/lib.typ": drawio
#drawio("<mxGraphModel>" + "<a>" * 600 + "</a>" * 600 + "</mxGraphModel>")
