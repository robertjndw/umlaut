# umlaut

draw.io and PlantUML diagrams for [Typst](https://typst.app), drawn with native Typst elements at compile time. No Java, no Graphviz install, no SVG round trip: the diagram becomes shapes and text in your document, in your fonts.

![Showcase](https://raw.githubusercontent.com/robertjndw/umlaut/main/docs/showcase.png)

```typ
#import "@local/umlaut:0.1.0": drawio, plantuml

#drawio(read("architecture.drawio"))
#plantuml(read("login.puml"))
```

A WebAssembly plugin turns both formats into rectangles, paths and text runs, which umlaut draws with Typst's own elements. For PlantUML, Typst measures every label in a first pass so boxes always fit their text, then the plugin lays the diagram out. Class, state, use case and component diagrams are placed and routed by [Graphviz](https://graphviz.org) dot, compiled into the plugin. The output is vector, selectable text, and follows your document font and `set text` rules.

## Installing from GitHub

umlaut isn't on Typst Universe yet. Until then you can install it as a local package:

macOS and Linux:

```sh
curl -fsSL https://raw.githubusercontent.com/robertjndw/umlaut/main/install.sh | sh
```

Windows (PowerShell):

```powershell
irm https://raw.githubusercontent.com/robertjndw/umlaut/main/install.ps1 | iex
```

Then import it from the `local` namespace:

```typ
#import "@local/umlaut:0.1.0": drawio, plantuml
```

The script copies the package to Typst's [local package directory](https://github.com/typst/packages#local-packages), or to `TYPST_PACKAGE_PATH` if that's set. For a specific tag or branch, use `sh -s -- --ref v0.1.0` (on Windows, set `$env:UMLAUT_REF` first). In a clone, `make install` installs your working tree.

## Usage

Packages can't read files from your project, so you pass the file's contents with `read()`:

```typ
#drawio(read("architecture.drawio"))
#plantuml(read("login.puml"))
```

If you'd rather write `#drawio("architecture.drawio")`, define a helper in your own file. `read()` there resolves paths relative to your file:

```typ
#import "@local/umlaut:0.1.0"
#let drawio(path, ..args) = umlaut.drawio(read(path), ..args)
#let plantuml(path, ..args) = umlaut.plantuml(read(path), ..args)
```

Sources can also be plain strings, so inline diagrams work too:

```typ
#plantuml("Alice -> Bob : hello")
```

### draw.io

Use the `.drawio` file as saved by draw.io (or diagrams.net), compressed or not. A `.drawio.svg` works as well, since draw.io embeds the diagram in it.

```typ
#drawio(read("architecture.drawio"), width: 100%)
```

![draw.io](https://raw.githubusercontent.com/robertjndw/umlaut/main/docs/drawio.png)

The source for this picture is [`examples/diagrams/architecture.drawio`](examples/diagrams/architecture.drawio).

Files with several pages draw the first one. Pick another by index or by name:

```typ
#drawio(read("pages.drawio"), page: 1)
#drawio(read("pages.drawio"), page: "Details")
```

![Pages](https://raw.githubusercontent.com/robertjndw/umlaut/main/docs/drawio-pages.png)

### PlantUML

Pass the source with or without `@startuml` / `@enduml`. The diagram type is detected from the statements in it.

**Sequence**

```typ
#plantuml(read("login.puml"))
```

![Sequence diagram](https://raw.githubusercontent.com/robertjndw/umlaut/main/docs/plantuml-sequence.png)

**Activity** (the current syntax with `start`, `:action;`, `if`, `fork`, ...)

![Activity diagram](https://raw.githubusercontent.com/robertjndw/umlaut/main/docs/plantuml-activity.png)

**Class**

![Class diagram](https://raw.githubusercontent.com/robertjndw/umlaut/main/docs/plantuml-class.png)

**State**

![State diagram](https://raw.githubusercontent.com/robertjndw/umlaut/main/docs/plantuml-state.png)

**Use case**

![Use case diagram](https://raw.githubusercontent.com/robertjndw/umlaut/main/docs/plantuml-usecase.png)

**Component and deployment**

![Component diagram](https://raw.githubusercontent.com/robertjndw/umlaut/main/docs/plantuml-component.png)

The sources of all of these are in [`examples/diagrams/`](examples/diagrams).

### Inline PlantUML

A show rule turns fenced `plantuml` blocks into diagrams:

```typ
#show raw.where(lang: "plantuml"): it => plantuml(it.text)
```

The diagram then uses the raw block's monospace font. Pass `font:` in the rule if you want something else.

### Size, scale and fonts

```typ
#plantuml(src, width: 100%)        // fill the available width, keep proportions
#plantuml(src, width: 8cm)
#plantuml(src, scale: 50%)         // half the natural size
#plantuml(src, font: "Libertinus Serif")
```

![Styling](https://raw.githubusercontent.com/robertjndw/umlaut/main/docs/styling.png)

Stroke widths and text scale together with the diagram. Without `width` or `height` a diagram has its natural size: draw.io pixels become points, and PlantUML diagrams are sized from your text size.

### Warnings

By default nothing makes compilation fail just because the result is imperfect. Two kinds of problems are collected as warnings:

- things the renderer could not draw faithfully: an unknown draw.io stencil, an embedded image, `!theme`, `skinparam handwritten`, ...
- layout problems in class, state, use case and component diagrams: a line running through a box or across a container's title, two lines drawn over each other, or a label that touches a box or sits closer to another line than to its own. Each one names the elements and the source line of the link, e.g. `layout: link "Bus" -> "Search" on line 46 and link "Search" -> "SI" on line 47 are drawn over each other for 104 pt`.

Warnings are attached as metadata, which you can list:

```typ
#drawio(read("aws.drawio"))

#context {
  for found in query(<umlaut-warnings>) {
    for message in found.value [- #message]
  }
}
```

To make sure no problem goes unnoticed, pass `warn: "error"`: any warning then fails compilation and the message lists all of them. Set it once in your helper to cover every diagram:

```typ
#let plantuml(path, ..args) = umlaut.plantuml(read(path), warn: "error", ..args)
```

A layout problem can often be fixed in the source: change a link's length (`-->` vs `--->`) or direction (`-up->`, `-left->`), reorder declarations, or try `skinparam linetype ortho`. Pass `warn: false` to attach nothing. Broken XML, PlantUML syntax errors and unsupported diagram types are always errors, with a line number where there is one.

## Reference

```typ
#drawio(
  source,        // str or bytes: contents of a .drawio or .drawio.svg file
  page: 0,       // index or name, for files with several pages
  width: auto,   // length, ratio or both (100% - 2cm); auto uses the size in the file
  height: auto,  // like width; a ratio needs a container with a fixed height
  scale: 100%,   // on top of width and height
  font: auto,    // font name or array of names for all text; auto keeps the diagram's fonts
  warn: true,    // attach <umlaut-warnings> metadata; "error" fails on any warning; false drops them
)

#plantuml(
  source,        // str or bytes
  width: auto,   // auto uses the laid-out size
  height: auto,
  scale: 100%,
  font: auto,    // auto uses the document font
  warn: true,    // like drawio; also reports layout problems
)
```

With both `width` and `height` the diagram fits inside the box and keeps its proportions.

## Supported features

### draw.io

- Shapes: rectangle (also rounded), ellipse, double ellipse, rhombus, triangle, hexagon, parallelogram, trapezoid, step, cylinder, data store, cloud, document, process, note, card, callout, internal storage, folder, cube, actor, UML actor, UML lifeline, swimlane and table, text, line
- Containers, groups, nested swimlanes, collapsed and hidden cells
- Edges: straight, orthogonal, elbow, entity relation, curved, with waypoints, dashed, labels and edge label children
- Markers: classic, block, open, oval, diamond, dash, cross, async, box, half circle, ER notation (one, many, mandatory one, zero to one, ...), filled or open, thin variants
- Labels: plain and HTML (bold, italic, underline, strike-through, font, size and color, line breaks, `div` blocks and lists), wrapping, alignment, label position, rotation
- Fill, stroke, dashes, gradients, opacity, rotation, flips
- Multiple pages, compressed pages, bare `<mxGraphModel>` files, `.drawio.svg`

Not supported yet:

- Embedded images (drawn as a placeholder box)
- Stencil libraries (`mxgraph.*` shapes, AWS, Azure, Cisco, ...). They become a dashed box with the shape name
- Sketch style (drawn clean) and shadows
- Isometric edges (routed orthogonally)

### PlantUML

| Diagram | Covered |
| --- | --- |
| Sequence | participants of all kinds, messages and arrow variants, activation, create and destroy, groups (`alt`, `opt`, `loop`, `par`, ...), dividers, notes, `ref over`, boxes, autonumber, messages from and to the diagram border |
| Activity | actions, `if` / `elseif` / `else`, `while`, `repeat`, `switch`, `fork` and `split`, partitions, swimlanes, notes, `detach` / `kill` |
| Class | classes, interfaces, enums, abstract classes, members with visibility, packages and namespaces, all relation types, labels and multiplicities, notes, stereotypes, generics, lollipop interfaces, `hide` / `show` |
| State | simple and composite states, concurrent regions, initial, final, choice, fork/join and history pseudo states, notes |
| Use case | actors, use cases, system boundaries (`rectangle`, `package`), `include` / `extend`, generalization |
| Component and deployment | components, interfaces, nodes, clouds, databases, queues and other containers, packages |

Shared by all types: `title`, `header`, `footer`, `caption`, `legend`, `skinparam`, colors and `<<stereotypes>>`, Creole markup in labels, `left to right direction`, `@startuml` and `@enduml` optional.

Not supported:

- The preprocessor: `!include`, `!define`, `!procedure`, `!if`, ... (an error). `!theme` and `!pragma` are ignored with a warning, use `skinparam` instead
- Object, timing, Gantt, mind map, WBS, wireframe (salt), JSON, YAML, ER and the other diagram families (an error naming the type)
- `newpage` in sequence diagrams, `scale` (use the `scale` argument of `plantuml()`), `skinparam handwritten`

## Building from source

```sh
make tools      # wasi-sdk and wasi-stub into .tools/ (nothing is installed system-wide)
make wasm       # rebuild umlaut.wasm
make test       # rebuild and run all tests
make examples   # render the pictures in docs/ from examples/
```

You need Rust with the `wasm32-unknown-unknown` target, `curl`, `tar`, [binaryen](https://github.com/WebAssembly/binaryen) (`wasm-opt`) and the Typst CLI. The first cargo build downloads the pinned Graphviz sources into `.deps/`; set `UMLAUT_GRAPHVIZ_SRC` to an unpacked Graphviz 14.1.4 tree to build offline. Both `.tools/` and `.deps/` are gitignored.

The Typst code is in `src/`, the Rust plugin in `plugin/`. `umlaut.wasm` is checked in because the package ships it, so run `make wasm` after changing anything in `plugin/`.

## Licenses

umlaut is MIT licensed. Graphviz's dot engine is compiled into `umlaut.wasm` and is licensed under the Eclipse Public License 2.0; see [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md) for the notice and the source location.
