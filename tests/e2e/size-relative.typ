// Ratios and relative lengths resolve against the space around the diagram. Each
// assertion measures the drawn box, so a wrong resolution fails the compile.
#import "../../src/lib.typ": drawio, plantuml
#set page(width: 300pt, height: auto, margin: 0pt)
#let src = read("../../plugin/tests/fixtures/drawio/bare.drawio")

// `measure` lays the diagram out in a region of the given size, like a container would.
#let check(body, w: none, h: none, within: (200pt, 100pt)) = context {
  let m = measure(width: within.at(0), height: within.at(1), body)
  if w != none { assert(calc.abs((m.width - w) / 1pt) < 0.5, message: "width " + repr(m.width) + " != " + repr(w)) }
  if h != none { assert(calc.abs((m.height - h) / 1pt) < 0.5, message: "height " + repr(m.height) + " != " + repr(h)) }
}

#check(drawio(src, width: 100%), w: 200pt)
#check(drawio(src, width: 50%), w: 100pt)
#check(drawio(src, width: 100% - 20pt), w: 180pt)
#check(drawio(src, width: -10pt + 50%), w: 90pt)
#check(plantuml("A -> B", width: 50%), w: 100pt)
#check(drawio(src, height: 50%), h: 50pt)
#check(drawio(src, width: 100%, height: 25%), h: 25pt)
// em resolves against the text size, and a negative part is fine when the sum is positive.
#set text(size: 10pt)
#check(drawio(src, width: -1pt + 3em), w: 29pt)
// Plain lengths keep working, and so does a real container.
#check(drawio(src, width: 40pt), w: 40pt)
#block(width: 120pt, drawio(src, width: 100%))
