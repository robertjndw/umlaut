// expect: this is not a draw.io file: expected <mxfile>, <mxGraphModel> or an SVG, found <html>
#import "../../src/lib.typ": drawio
#drawio("<html><body>hello</body></html>")
