// expect: the compressed page is not valid base64
#import "../../src/lib.typ": drawio
#drawio("<mxfile><diagram name=\"p\">%%% definitely not base64 %%%</diagram></mxfile>")
