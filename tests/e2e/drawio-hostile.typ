// Values that used to reach Typst as invalid colors, NaN coordinates or unknown
// font families. The document only has to compile.
#import "../../src/lib.typ": drawio
#set page(width: auto, height: auto, margin: 8pt)
#drawio(read("drawio-hostile.drawio"))
