// Draws a decoded scene (layout documented in plugin/src/scene.rs) with native Typst
// elements. Coordinates are in pt with the origin at the top left; `k` is the uniform
// factor applied to coordinates, stroke widths and font sizes.

#let _pt(v) = v * 1pt

#let _color(hex, opacity) = {
  let c = rgb(hex)
  if opacity < 1 { c = c.transparentize((1 - opacity) * 100%) }
  c
}

#let _paint(p, opacity) = {
  if p == none { return none }
  let c = _color(p.color, opacity)
  if p.gradient == none { c } else {
    gradient.linear(c, _color(p.gradient.color, opacity), angle: p.gradient.angle * 1deg)
  }
}

#let _stroke(s, opacity, k) = {
  if s == none or s.width <= 0 { return none }
  stroke(
    paint: _paint(s.paint, opacity),
    thickness: _pt(s.width * k),
    dash: if s.dash.len() > 0 { s.dash.map(d => _pt(d * k)) } else { none },
    cap: s.cap,
    join: s.join,
  )
}

// The fonts of the surrounding document, so a font the diagram asks for can fall back
// to it. Needs context.
#let doc-fonts() = {
  let f = text.font
  if type(f) == array { f } else { (f,) }
}

// Content for a list of runs. With `nowrap`, spaces become non-breaking so a line only
// ends at an explicit break. Shared by drawing and by PlantUML's measuring pass, so
// the measured size is the drawn size.
#let runs-content(runs, k: 1.0, font: auto, doc-fonts: (), nowrap: false) = {
  set par(leading: 0.3em, spacing: 0pt, justify: false, first-line-indent: 0pt, hanging-indent: 0pt)
  for r in runs {
    if r.br {
      linebreak()
      continue
    }
    let args = (size: _pt(r.size * k), fill: _color(r.color, 1))
    if r.bold { args.weight = "bold" }
    if r.italic { args.style = "italic" }
    // Unknown families only trigger a compiler warning, and the document fonts
    // behind the requested one keep the text visible.
    let family = if font != auto { font } else if r.font != none { (r.font,) } else { none }
    if family != none { args.font = (..family, ..doc-fonts) }
    let t = if nowrap { r.text.replace(" ", "\u{a0}") } else { r.text }
    let c = std.text(..args, t)
    if r.underline { c = underline(c) }
    if r.strike { c = strike(c) }
    if r.link != none { c = link(r.link, c) }
    c
  }
}

#let _rotated(deg, body) = if deg == 0 { body } else {
  rotate(deg * 1deg, origin: center + horizon, body)
}

#let _draw-shape(it, k) = {
  let op = it.opacity
  let fill = _paint(it.fill, op)
  let stroke = _stroke(it.stroke, op, k)
  let (bx, by, bw, bh) = it.bbox.map(v => v * k)
  let g = it.geom
  let inner = if g.k == "rect" {
    rect(width: _pt(bw), height: _pt(bh), radius: _pt(g.radius * k), fill: fill, stroke: stroke)
  } else if g.k == "ellipse" {
    ellipse(width: _pt(bw), height: _pt(bh), fill: fill, stroke: stroke)
  } else if g.k == "polygon" {
    if g.points.len() < 2 { return }
    polygon(fill: fill, stroke: stroke, ..g.points.map(p => (_pt(p.at(0) * k - bx), _pt(p.at(1) * k - by))))
  } else {
    // Points are relative to the item's own box so the curve never has negative
    // coordinates.
    let at(x, y) = (_pt(x * k - bx), _pt(y * k - by))
    curve(fill: fill, stroke: stroke, ..g.cmds.map(c => {
      if c.c == "m" { curve.move(at(c.x, c.y)) }
      else if c.c == "l" { curve.line(at(c.x, c.y)) }
      else if c.c == "c" { curve.cubic(at(c.x1, c.y1), at(c.x2, c.y2), at(c.x, c.y)) }
      else { curve.close(mode: "straight") }
    }))
  }
  place(top + left, dx: _pt(bx), dy: _pt(by), _rotated(it.rotation, box(width: _pt(bw), height: _pt(bh), inner)))
}

#let _draw-text(it, k, font, doc-fonts) = {
  let ha = (left: left, center: center, right: right).at(it.halign)
  let va = (top: top, middle: horizon, bottom: bottom).at(it.valign)
  let body = runs-content(it.runs, k: k, font: font, doc-fonts: doc-fonts, nowrap: not it.wrap)
  let b = box(
    width: _pt(it.w * k),
    height: _pt(it.h * k),
    fill: _paint(it.background, 1),
    align(ha + va, block(width: 100%, body)),
  )
  place(top + left, dx: _pt(it.x * k), dy: _pt(it.y * k), _rotated(it.rotation, b))
}

/// Draws a scene at factor `k` (1 = one scene pt per Typst pt).
#let scene-box(scene, k, font: auto, doc-fonts: ()) = box(
  width: _pt(scene.size.at(0) * k),
  height: _pt(scene.size.at(1) * k),
  {
    for it in scene.items {
      if it.t == "shape" { _draw-shape(it, k) } else { _draw-text(it, k, font, doc-fonts) }
    }
  },
)

// Whether a size depends on the space around the diagram.
#let _relative(value) = type(value) == ratio or type(value) == relative

// Turns a validated size into a plain length. `available` is the container's extent
// along that axis; a ratio of an unbounded container (an `auto` page, say) has no
// meaning, so that is an error rather than a guess.
#let resolve-size(name, value, available) = {
  if value == auto { return auto }
  let resolved = if _relative(value) {
    if available / 1pt == float.inf {
      panic("umlaut: `" + name + "` is " + repr(value) + " of its container, but the container has no fixed " + name + " here; use a length instead")
    }
    let (part, share) = if type(value) == ratio { (0pt, value) } else { (value.length, value.ratio) }
    part.to-absolute() + available * (share / 100%)
  } else { value.to-absolute() }
  if resolved <= 0pt {
    panic("umlaut: `" + name + "` must be positive, but " + repr(value) + " resolves to " + repr(resolved))
  }
  resolved
}

// Factor from `width`, `height` and `scale`, both already plain lengths or `auto`. With
// both lengths the diagram fits inside them and keeps its proportions.
#let factor(scene, width, height, scale) = {
  let (w, h) = scene.size
  let k = 1.0
  if width != auto and w > 0 { k = width / 1pt / w }
  if height != auto and h > 0 {
    let kh = height / 1pt / h
    k = if width == auto { kh } else { calc.min(k, kh) }
  }
  k * scale
}

/// Draws a decoded scene according to the public `width`, `height`, `scale` and `font`
/// arguments. Warnings are attached as `<umlaut-warnings>` metadata when `warn` is
/// true, and fail compilation when it is "error".
/// Sizes may be lengths, ratios or relative lengths; ratios are taken against the space
/// the diagram is placed in.
#let diagram(scene, width: auto, height: auto, scale: 1.0, font: auto, warn: false) = {
  if warn == "error" and scene.warnings.len() > 0 {
    panic("umlaut: the diagram has problems (pass `warn: true` to draw it anyway):\n- " + scene.warnings.join("\n- "))
  }
  let draw(w, h) = {
    let k = factor(scene, w, h, scale)
    scene-box(scene, k, font: font, doc-fonts: doc-fonts())
    if warn == true and scene.warnings.len() > 0 [#metadata(scene.warnings) <umlaut-warnings>]
  }
  // `layout` makes the content block-level, so it is only used when a ratio needs the
  // container; plain lengths keep the diagram inline.
  if _relative(width) or _relative(height) {
    layout(size => context draw(
      resolve-size("width", width, size.width),
      resolve-size("height", height, size.height),
    ))
  } else {
    context draw(resolve-size("width", width, 0pt), resolve-size("height", height, 0pt))
  }
}
