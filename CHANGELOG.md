# Changelog

## 0.1.0

Initial release.

- `drawio` draws `.drawio` and `.drawio.svg` files with native Typst elements: shapes, containers and swimlanes, groups, orthogonal, elbow, curved and entity relation edges, arrow markers, HTML labels, gradients, rotation, multiple and compressed pages
- `page` picks a page of a multi-page draw.io file by index or name
- `plantuml` draws sequence, activity, class, state, use case and component/deployment diagrams, with `title`, `header`, `footer`, `legend` and `skinparam`
- PlantUML labels are measured by Typst in the diagram's font, so boxes always fit their text
- Class, state, use case and component diagrams are placed and routed by Graphviz dot, compiled into the plugin
- `width`, `height` (lengths, ratios or both), `scale` and `font` on both functions
- Things that can't be drawn faithfully (stencils, images, `!theme`, ...) are reported as `<umlaut-warnings>` metadata instead of failing; `warn: false` turns that off
- Graph diagrams are checked after layout, and lines through boxes or titles, lines drawn over each other and misplaced labels are reported as warnings naming the elements and source lines
- `warn: "error"` makes any warning fail compilation
- `install.sh` and `install.ps1` install the package as `@local/umlaut`
