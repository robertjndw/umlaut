#import "validate.typ"
#import "render.typ"

#let _plugin = plugin("../umlaut.wasm")

#let _u32(n) = n.to-bytes(endian: "little", size: 4)
#let _f32(x) = float(x).to-bytes(endian: "little", size: 4)

// Option bytes for the plugin; layouts documented in plugin/src/lib.rs.
#let _drawio-options(page) = if type(page) == int {
  bytes((0,)) + _u32(page)
} else {
  bytes((1,)) + _u32(0) + bytes(page)
}

/// Draws a draw.io diagram with native Typst elements.
///
/// Packages cannot read files from your project, so pass the file's contents:
/// `drawio(read("architecture.drawio"))`.
///
/// Anything the renderer could not draw faithfully (an unknown stencil, say) is
/// listed as `<umlaut-warnings>` metadata, which `query(<umlaut-warnings>)` can read
/// inside `context`; nothing shows up in the document itself.
///
/// - source (str, bytes): Contents of a `.drawio` or `.drawio.svg` file, compressed
///   or not.
/// - page (int, str): Page of a multi-page file, by index or by name.
/// - width (length, ratio, relative, auto): Width of the diagram. A ratio such as
///   `100%` or a mix like `100% - 2cm` is taken against the space the diagram is
///   placed in. `auto` uses the size in the file.
/// - height (length, ratio, relative, auto): Height of the diagram, resolved like
///   `width`; a ratio needs a container with a fixed height (a fixed-size `block` or
///   `box`, not an `auto`-sized page). With both `width` and `height` the diagram fits
///   inside and keeps its proportions.
/// - scale (ratio): Extra scaling on top of `width` and `height`. Stroke widths and
///   font sizes scale with it.
/// - font (str, array, auto): Font for all text. `auto` keeps the fonts the diagram
///   asks for and falls back to the document font.
/// - warn (bool, str): `true` attaches the warnings as `<umlaut-warnings>` metadata,
///   `"error"` makes any warning fail compilation, `false` drops them.
/// -> content
#let drawio(source, page: 0, width: auto, height: auto, scale: 100%, font: auto, warn: true) = {
  let payload = validate.source(source)
  let page = validate.page(page)
  let width = validate.size("width", width)
  let height = validate.size("height", height)
  let scale = validate.scale(scale)
  let font = validate.font(font)
  let warn = validate.warn(warn)

  let scene = cbor(_plugin.drawio(payload, _drawio-options(page)))
  render.diagram(scene, width: width, height: height, scale: scale, font: font, warn: warn)
}

// Packs measured sizes as f32 pairs; see `plantuml_layout` in plugin/src/lib.rs.
#let _pack-sizes(sizes) = bytes(sizes.map(s => array(_f32(s.at(0)) + _f32(s.at(1)))).flatten())

// The labels/layout round trip shared by the text diagram formats: the plugin lists
// the labels, Typst measures them in the final font, and the plugin lays out with those
// sizes. The arguments are already validated.
#let _two-pass(labels-fn, layout-fn, payload, width, height, scale, font, warn) = {
  let labels = cbor(labels-fn(payload))
  context {
    let fonts = render.doc-fonts()
    // Measured at scale 1: the layout works in unscaled pt and the whole result is
    // scaled afterwards.
    let sizes = labels.map(l => {
      let body = render.runs-content(l.runs, font: font, doc-fonts: fonts)
      let m = if l.wrap_width == none { measure(body) } else { measure(block(width: l.wrap_width * 1pt, body)) }
      (m.width / 1pt, m.height / 1pt)
    })
    let scene = cbor(layout-fn(payload, _pack-sizes(sizes)))
    render.diagram(scene, width: width, height: height, scale: scale, font: font, warn: warn)
  }
}

/// Draws a PlantUML diagram with native Typst elements.
///
/// The plugin lays the diagram out, using the real size of every label in the font
/// the diagram ends up in, so boxes always fit their text. Pass the source as text:
/// `plantuml(read("login.puml"))`.
///
/// - source (str, bytes): PlantUML source, with or without `@startuml`/`@enduml`.
/// - width (length, ratio, relative, auto): Width of the diagram. A ratio such as
///   `100%` or a mix like `100% - 2cm` is taken against the space the diagram is
///   placed in. `auto` uses the laid-out size.
/// - height (length, ratio, relative, auto): Height of the diagram, resolved like
///   `width`; a ratio needs a container with a fixed height (a fixed-size `block` or
///   `box`, not an `auto`-sized page). With both `width` and `height` the diagram fits
///   inside and keeps its proportions.
/// - scale (ratio): Extra scaling on top of `width` and `height`. Stroke widths and
///   font sizes scale with it.
/// - font (str, array, auto): Font for all text. `auto` uses the document font.
/// - warn (bool, str): What to do with what could not be drawn faithfully (`!theme`,
///   `skinparam handwritten`, ...) and with layout problems such as a line running
///   through a box or two lines drawn over each other. `true` attaches them as
///   `<umlaut-warnings>` metadata, `"error"` makes any of them fail compilation,
///   `false` drops them.
/// -> content
#let plantuml(source, width: auto, height: auto, scale: 100%, font: auto, warn: true) = {
  let payload = validate.source(source)
  let width = validate.size("width", width)
  let height = validate.size("height", height)
  let scale = validate.scale(scale)
  let font = validate.font(font)
  let warn = validate.warn(warn)

  _two-pass(_plugin.plantuml_labels, _plugin.plantuml_layout, payload, width, height, scale, font, warn)
}

/// Draws a Mermaid diagram with native Typst elements.
///
/// Flowcharts, sequence diagrams, class diagrams and state diagrams are supported, in
/// Mermaid's default theme. The plugin lays the diagram out, using the real size of
/// every label in the font the diagram ends up in, so boxes always fit their text.
/// Placement comes from Graphviz, so it differs from mermaid.js in details. Pass the
/// source as text: `mermaid(read("flow.mmd"))`.
///
/// - source (str, bytes): Mermaid source, with or without front matter.
/// - width (length, ratio, relative, auto): Width of the diagram. A ratio such as
///   `100%` or a mix like `100% - 2cm` is taken against the space the diagram is
///   placed in. `auto` uses the laid-out size.
/// - height (length, ratio, relative, auto): Height of the diagram, resolved like
///   `width`; a ratio needs a container with a fixed height (a fixed-size `block` or
///   `box`, not an `auto`-sized page). With both `width` and `height` the diagram fits
///   inside and keeps its proportions.
/// - scale (ratio): Extra scaling on top of `width` and `height`. Stroke widths and
///   font sizes scale with it.
/// - font (str, array, auto): Font for all text. `auto` uses the document font.
/// - warn (bool, str): What to do with what could not be drawn faithfully (`%%{init}%%`
///   directives, `click` statements, ...) and with layout problems such as a line
///   running through a box or two lines drawn over each other. `true` attaches them as
///   `<umlaut-warnings>` metadata, `"error"` makes any of them fail compilation,
///   `false` drops them.
/// -> content
#let mermaid(source, width: auto, height: auto, scale: 100%, font: auto, warn: true) = {
  let payload = validate.source(source)
  let width = validate.size("width", width)
  let height = validate.size("height", height)
  let scale = validate.scale(scale)
  let font = validate.font(font)
  let warn = validate.warn(warn)

  _two-pass(_plugin.mermaid_labels, _plugin.mermaid_layout, payload, width, height, scale, font, warn)
}

// Renders the plugin's hand-written fixture scene; used by the tests.
#let _debug-scene(..args) = render.diagram(cbor(_plugin.debug_scene()), ..args)
