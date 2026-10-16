//! The scene both the draw.io and the PlantUML front ends produce, and `src/render.typ`
//! draws. Everything is in points with the origin at the top left, y pointing down.
//!
//! The scene is serialized to CBOR and decoded in Typst with `cbor()`. Optional values
//! are always present as keys and are `null` (Typst `none`) when unset, so the Typst
//! side never needs `.at(.., default:)`. Layout of the decoded value:
//!
//! ```text
//! scene   = (size: (w, h), items: (item, ..), warnings: (str, ..))
//! item    = shape | text            // drawn in order, later items on top
//!
//! shape   = (t: "shape", geom, bbox: (x, y, w, h), fill: paint|none, stroke: stroke|none,
//!            opacity: 0..1, rotation: deg)
//! geom    = (k: "rect", x, y, w, h, radius)
//!         | (k: "ellipse", x, y, w, h)          // inscribed in the box
//!         | (k: "polygon", points: ((x, y), ..)) // closed
//!         | (k: "path", cmds: (cmd, ..))
//! cmd     = (c: "m", x, y) | (c: "l", x, y) | (c: "c", x1, y1, x2, y2, x, y) | (c: "z")
//! paint   = (color: "#rrggbbaa", gradient: (color: "#rrggbbaa", angle: deg)|none)
//!                                            // linear, color -> gradient.color; angle in
//!                                            // Typst's convention (0 = left to right,
//!                                            // 90 = top to bottom)
//! stroke  = (paint, width, dash: (f32, ..), cap: "butt"|"round"|"square",
//!            join: "miter"|"round"|"bevel")  // dash holds alternating dash/gap lengths
//!
//! text    = (t: "text", x, y, w, h, halign: "left"|"center"|"right",
//!            valign: "top"|"middle"|"bottom", wrap: bool, rotation: deg,
//!            background: paint|none, runs: (run, ..))
//! run     = (text, bold, italic, underline, strike, size, color: "#rrggbbaa",
//!            font: str|none, link: str|none, br: bool)
//!                                            // br: true is a forced line break, the
//!                                            // other fields are then ignored
//! ```
//!
//! `bbox` is the control-point bounding box of the geometry (the builder fills it in);
//! rotation turns the item around the center of that box (or of the text box).
//! Arrowheads and other markers have no scene primitive: producers emit them as
//! polygons or paths.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    pub size: [f32; 2],
    pub items: Vec<Item>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum Item {
    Shape(Shape),
    Text(TextBox),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Shape {
    pub geom: Geom,
    pub bbox: [f32; 4],
    pub fill: Option<Paint>,
    pub stroke: Option<Stroke>,
    pub opacity: f32,
    pub rotation: f32,
}

impl Shape {
    pub fn new(geom: Geom, bbox: [f32; 4], style: Style) -> Shape {
        Shape { geom, bbox, fill: style.fill, stroke: style.stroke, opacity: style.opacity, rotation: style.rotation }
    }

    pub fn rect(x: f32, y: f32, w: f32, h: f32, radius: f32, style: Style) -> Shape {
        Shape::new(Geom::Rect { x, y, w, h, radius }, [x, y, w, h], style)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "k", rename_all = "lowercase")]
pub enum Geom {
    Rect { x: f32, y: f32, w: f32, h: f32, radius: f32 },
    Ellipse { x: f32, y: f32, w: f32, h: f32 },
    Polygon { points: Vec<[f32; 2]> },
    Path { cmds: Vec<PathCmd> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "c")]
pub enum PathCmd {
    #[serde(rename = "m")]
    Move { x: f32, y: f32 },
    #[serde(rename = "l")]
    Line { x: f32, y: f32 },
    #[serde(rename = "c")]
    Cubic { x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32 },
    #[serde(rename = "z")]
    Close,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Paint {
    /// `#rrggbbaa`.
    pub color: String,
    pub gradient: Option<Gradient>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Gradient {
    pub color: String,
    pub angle: f32,
}

impl Paint {
    /// Accepts `#rgb`, `#rrggbb` and `#rrggbbaa` (with or without the `#`) and
    /// normalizes to `#rrggbbaa`; anything else gives opaque black (see `hex`).
    pub fn solid(color: &str) -> Paint {
        Paint { color: hex(color), gradient: None }
    }

    pub fn rgba(r: u8, g: u8, b: u8, a: u8) -> Paint {
        Paint { color: format!("#{r:02x}{g:02x}{b:02x}{a:02x}"), gradient: None }
    }

    /// Linear gradient from `self` to `to`; `angle` follows Typst (90 = downwards).
    pub fn gradient_to(mut self, to: &str, angle: f32) -> Paint {
        self.gradient = Some(Gradient { color: hex(to), angle });
        self
    }
}

/// Normalizes a hex color to `#rrggbbaa`. Input that is not a hex color becomes opaque
/// black: callers pass colors they already checked (or constants), and the renderer
/// must never see a string Typst's `rgb()` would reject.
pub fn hex(color: &str) -> String {
    crate::color::normalize(color).unwrap_or_else(|| "#000000ff".to_string())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    pub paint: Paint,
    pub width: f32,
    pub dash: Vec<f32>,
    pub cap: Cap,
    pub join: Join,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Cap {
    Butt,
    Round,
    Square,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Join {
    Miter,
    Round,
    Bevel,
}

impl Stroke {
    pub fn new(paint: Paint, width: f32) -> Stroke {
        Stroke { paint, width, dash: Vec::new(), cap: Cap::Butt, join: Join::Miter }
    }

    pub fn dash(mut self, dash: Vec<f32>) -> Stroke {
        self.dash = dash;
        self
    }

    pub fn cap(mut self, cap: Cap) -> Stroke {
        self.cap = cap;
        self
    }

    pub fn join(mut self, join: Join) -> Stroke {
        self.join = join;
        self
    }
}

/// Fill, stroke, opacity and rotation of a shape; passed to the `Scene::push_*` calls.
#[derive(Debug, Clone, PartialEq)]
pub struct Style {
    pub fill: Option<Paint>,
    pub stroke: Option<Stroke>,
    pub opacity: f32,
    pub rotation: f32,
}

impl Default for Style {
    fn default() -> Self {
        Style { fill: None, stroke: None, opacity: 1.0, rotation: 0.0 }
    }
}

impl Style {
    pub fn new() -> Style {
        Style::default()
    }

    pub fn fill(mut self, fill: Paint) -> Style {
        self.fill = Some(fill);
        self
    }

    pub fn stroke(mut self, stroke: Stroke) -> Style {
        self.stroke = Some(stroke);
        self
    }

    /// 0 (invisible) to 1; applied to fill and stroke together.
    pub fn opacity(mut self, opacity: f32) -> Style {
        self.opacity = opacity.clamp(0.0, 1.0);
        self
    }

    pub fn rotation(mut self, degrees: f32) -> Style {
        self.rotation = degrees;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HAlign {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VAlign {
    Top,
    Middle,
    Bottom,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Run {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    /// Font size in pt.
    pub size: f32,
    pub color: String,
    /// Family name the source asked for; the renderer falls back to the document font.
    pub font: Option<String>,
    pub link: Option<String>,
    pub br: bool,
}

impl Run {
    pub fn new(text: impl Into<String>) -> Run {
        Run {
            text: text.into(),
            bold: false,
            italic: false,
            underline: false,
            strike: false,
            size: 12.0,
            color: "#000000ff".into(),
            font: None,
            link: None,
            br: false,
        }
    }

    /// A forced line break.
    pub fn br() -> Run {
        Run { br: true, ..Run::new("") }
    }

    pub fn bold(mut self) -> Run {
        self.bold = true;
        self
    }

    pub fn italic(mut self) -> Run {
        self.italic = true;
        self
    }

    pub fn underline(mut self) -> Run {
        self.underline = true;
        self
    }

    pub fn strike(mut self) -> Run {
        self.strike = true;
        self
    }

    pub fn size(mut self, size: f32) -> Run {
        self.size = size;
        self
    }

    pub fn color(mut self, color: &str) -> Run {
        self.color = hex(color);
        self
    }

    pub fn font(mut self, font: impl Into<String>) -> Run {
        self.font = Some(font.into());
        self
    }

    pub fn link(mut self, url: impl Into<String>) -> Run {
        self.link = Some(url.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextBox {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub halign: HAlign,
    pub valign: VAlign,
    /// Whether lines break at the box width. Without it the renderer keeps each line
    /// intact and lets it overflow the box.
    pub wrap: bool,
    pub rotation: f32,
    pub background: Option<Paint>,
    pub runs: Vec<Run>,
}

impl TextBox {
    /// Centered both ways, wrapping, no rotation or background.
    pub fn new(x: f32, y: f32, w: f32, h: f32, runs: Vec<Run>) -> TextBox {
        TextBox {
            x,
            y,
            w,
            h,
            halign: HAlign::Center,
            valign: VAlign::Middle,
            wrap: true,
            rotation: 0.0,
            background: None,
            runs,
        }
    }

    pub fn align(mut self, h: HAlign, v: VAlign) -> TextBox {
        self.halign = h;
        self.valign = v;
        self
    }

    pub fn wrap(mut self, wrap: bool) -> TextBox {
        self.wrap = wrap;
        self
    }

    pub fn rotation(mut self, degrees: f32) -> TextBox {
        self.rotation = degrees;
        self
    }

    pub fn background(mut self, paint: Paint) -> TextBox {
        self.background = Some(paint);
        self
    }
}

/// Builds the `cmds` of a path shape.
#[derive(Debug, Clone, Default)]
pub struct PathBuilder {
    cmds: Vec<PathCmd>,
}

impl PathBuilder {
    pub fn new() -> PathBuilder {
        PathBuilder::default()
    }

    pub fn move_to(&mut self, x: f32, y: f32) -> &mut Self {
        self.cmds.push(PathCmd::Move { x, y });
        self
    }

    pub fn line_to(&mut self, x: f32, y: f32) -> &mut Self {
        self.cmds.push(PathCmd::Line { x, y });
        self
    }

    pub fn cubic_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) -> &mut Self {
        self.cmds.push(PathCmd::Cubic { x1, y1, x2, y2, x, y });
        self
    }

    /// Quadratic curve, converted to the cubic the scene knows. Needs the current
    /// point, so a `move_to` has to come first.
    pub fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) -> &mut Self {
        let (x0, y0) = self.current().unwrap_or((x1, y1));
        self.cubic_to(
            x0 + 2.0 / 3.0 * (x1 - x0),
            y0 + 2.0 / 3.0 * (y1 - y0),
            x + 2.0 / 3.0 * (x1 - x),
            y + 2.0 / 3.0 * (y1 - y),
            x,
            y,
        )
    }

    pub fn close(&mut self) -> &mut Self {
        self.cmds.push(PathCmd::Close);
        self
    }

    pub fn current(&self) -> Option<(f32, f32)> {
        self.cmds.iter().rev().find_map(|c| match *c {
            PathCmd::Move { x, y } | PathCmd::Line { x, y } | PathCmd::Cubic { x, y, .. } => Some((x, y)),
            PathCmd::Close => None,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.cmds.is_empty()
    }
}

fn bounds(points: impl Iterator<Item = (f32, f32)>) -> [f32; 4] {
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for (x, y) in points {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    if x0 > x1 {
        return [0.0; 4];
    }
    [x0, y0, x1 - x0, y1 - y0]
}

impl Scene {
    pub fn new(width: f32, height: f32) -> Scene {
        Scene { size: [width, height], items: Vec::new(), warnings: Vec::new() }
    }

    fn push_shape(&mut self, geom: Geom, bbox: [f32; 4], style: Style) {
        self.items.push(Item::Shape(Shape::new(geom, bbox, style)));
    }

    pub fn push_rect(&mut self, x: f32, y: f32, w: f32, h: f32, radius: f32, style: Style) {
        self.push_shape(Geom::Rect { x, y, w, h, radius }, [x, y, w, h], style);
    }

    /// Ellipse inscribed in the box.
    pub fn push_ellipse(&mut self, x: f32, y: f32, w: f32, h: f32, style: Style) {
        self.push_shape(Geom::Ellipse { x, y, w, h }, [x, y, w, h], style);
    }

    pub fn push_polygon(&mut self, points: &[(f32, f32)], style: Style) {
        let bbox = bounds(points.iter().copied());
        let points = points.iter().map(|&(x, y)| [x, y]).collect();
        self.push_shape(Geom::Polygon { points }, bbox, style);
    }

    /// Open paths are fine, the fill closes them implicitly. Empty paths are dropped.
    pub fn push_path(&mut self, path: PathBuilder, style: Style) {
        if path.is_empty() {
            return;
        }
        // The control points enclose the curve, which is close enough for the box the
        // renderer rotates around and places the path in.
        let bbox = bounds(path.cmds.iter().flat_map(|c| match *c {
            PathCmd::Move { x, y } | PathCmd::Line { x, y } => vec![(x, y)],
            PathCmd::Cubic { x1, y1, x2, y2, x, y } => vec![(x1, y1), (x2, y2), (x, y)],
            PathCmd::Close => vec![],
        }));
        self.push_shape(Geom::Path { cmds: path.cmds }, bbox, style);
    }

    /// Straight line or polyline without fill.
    pub fn push_polyline(&mut self, points: &[(f32, f32)], stroke: Stroke) {
        let mut path = PathBuilder::new();
        for (i, &(x, y)) in points.iter().enumerate() {
            if i == 0 {
                path.move_to(x, y);
            } else {
                path.line_to(x, y);
            }
        }
        self.push_path(path, Style::new().stroke(stroke));
    }

    pub fn push_text(&mut self, text: TextBox) {
        self.items.push(Item::Text(text));
    }

    /// Records something that could not be drawn faithfully; duplicates are kept once.
    pub fn warn(&mut self, message: impl Into<String>) {
        let message = message.into();
        if !self.warnings.contains(&message) {
            self.warnings.push(message);
        }
    }

    /// Moves every item by (dx, dy).
    pub fn translate(&mut self, dx: f32, dy: f32) {
        translate_items(&mut self.items, dx, dy);
    }

    /// Drops or repairs everything `render.typ` could choke on: non-finite numbers and
    /// color strings that are not `#rrggbbaa`. Producers are expected to emit clean
    /// values, but a hostile input must never turn into a Typst error, so this is the
    /// last line of defence before encoding.
    pub fn sanitize(&mut self) {
        for v in &mut self.size {
            if !v.is_finite() || *v < 0.0 {
                *v = 0.0;
            }
        }
        self.items.retain_mut(Item::sanitize);
    }

    pub fn to_cbor(&self) -> Result<Vec<u8>, String> {
        let mut clean = self.clone();
        clean.sanitize();
        let mut out = Vec::new();
        ciborium::into_writer(&clean, &mut out).map_err(|e| format!("could not encode the scene: {e}"))?;
        Ok(out)
    }

    pub fn from_cbor(bytes: &[u8]) -> Result<Scene, String> {
        ciborium::from_reader(bytes).map_err(|e| format!("could not decode the scene: {e}"))
    }
}

/// Moves items by (dx, dy).
pub fn translate_items(items: &mut [Item], dx: f32, dy: f32) {
    for item in items {
        item.translate(dx, dy);
    }
}

/// Plain text of the runs, line breaks as `\n`; for assertions and error messages.
pub fn plain_text(runs: &[Run]) -> String {
    runs.iter().map(|r| if r.br { "\n" } else { r.text.as_str() }).collect()
}

/// Position of the next occurrence of `seq` in `chars[from..]`.
pub(crate) fn find_seq(chars: &[char], from: usize, seq: &[char]) -> Option<usize> {
    (from..chars.len().saturating_sub(seq.len() - 1)).find(|&i| chars[i..].starts_with(seq))
}

fn all_finite(values: &[f32]) -> bool {
    values.iter().all(|v| v.is_finite())
}

impl Paint {
    /// Fixes the paint in place; `false` when the main color is unusable and the paint
    /// has to go.
    fn sanitize(&mut self) -> bool {
        if !crate::color::is_valid_output(&self.color) {
            return false;
        }
        if let Some(g) = &mut self.gradient {
            if !crate::color::is_valid_output(&g.color) {
                self.gradient = None;
            } else if !g.angle.is_finite() {
                g.angle = 0.0;
            }
        }
        true
    }
}

impl Geom {
    fn is_finite(&self) -> bool {
        match self {
            Geom::Rect { x, y, w, h, radius } => all_finite(&[*x, *y, *w, *h, *radius]),
            Geom::Ellipse { x, y, w, h } => all_finite(&[*x, *y, *w, *h]),
            Geom::Polygon { points } => points.iter().all(|p| all_finite(p)),
            Geom::Path { cmds } => cmds.iter().all(|c| match *c {
                PathCmd::Move { x, y } | PathCmd::Line { x, y } => all_finite(&[x, y]),
                PathCmd::Cubic { x1, y1, x2, y2, x, y } => all_finite(&[x1, y1, x2, y2, x, y]),
                PathCmd::Close => true,
            }),
        }
    }
}

impl Item {
    pub fn translate(&mut self, dx: f32, dy: f32) {
        match self {
            Item::Text(t) => {
                t.x += dx;
                t.y += dy;
            }
            Item::Shape(s) => {
                s.bbox[0] += dx;
                s.bbox[1] += dy;
                match &mut s.geom {
                    Geom::Rect { x, y, .. } | Geom::Ellipse { x, y, .. } => {
                        *x += dx;
                        *y += dy;
                    }
                    Geom::Polygon { points } => {
                        for p in points {
                            p[0] += dx;
                            p[1] += dy;
                        }
                    }
                    Geom::Path { cmds } => {
                        for c in cmds {
                            match c {
                                PathCmd::Move { x, y } | PathCmd::Line { x, y } => {
                                    *x += dx;
                                    *y += dy;
                                }
                                PathCmd::Cubic { x1, y1, x2, y2, x, y } => {
                                    *x1 += dx;
                                    *y1 += dy;
                                    *x2 += dx;
                                    *y2 += dy;
                                    *x += dx;
                                    *y += dy;
                                }
                                PathCmd::Close => {}
                            }
                        }
                    }
                }
            }
        }
    }

    /// Repairs the item in place; `false` means it cannot be drawn and should be dropped.
    fn sanitize(&mut self) -> bool {
        match self {
            Item::Shape(s) => {
                if !s.geom.is_finite() || !all_finite(&s.bbox) {
                    return false;
                }
                if !s.opacity.is_finite() {
                    s.opacity = 1.0;
                }
                s.opacity = s.opacity.clamp(0.0, 1.0);
                if !s.rotation.is_finite() {
                    s.rotation = 0.0;
                }
                if let Some(p) = &mut s.fill {
                    if !p.sanitize() {
                        s.fill = None;
                    }
                }
                if let Some(st) = &mut s.stroke {
                    let ok = st.paint.sanitize() && st.width.is_finite() && st.width >= 0.0;
                    if !ok {
                        s.stroke = None;
                    } else if !all_finite(&st.dash) || st.dash.iter().any(|d| *d < 0.0) {
                        st.dash.clear();
                    }
                }
                true
            }
            Item::Text(t) => {
                if !all_finite(&[t.x, t.y, t.w, t.h]) {
                    return false;
                }
                if !t.rotation.is_finite() {
                    t.rotation = 0.0;
                }
                if let Some(p) = &mut t.background {
                    if !p.sanitize() {
                        t.background = None;
                    }
                }
                for r in &mut t.runs {
                    if !r.size.is_finite() || r.size <= 0.0 {
                        r.size = 12.0;
                    }
                    if !crate::color::is_valid_output(&r.color) {
                        r.color = "#000000ff".into();
                    }
                }
                true
            }
        }
    }
}

/// A piece of text the PlantUML layout needs measured: Typst reports how big the runs
/// come out in the real font before `plantuml::layout` runs. `wrap_width` is the width
/// the text may not exceed (it wraps there); `None` means as wide as it wants, with
/// only the explicit `br` runs breaking lines.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Label {
    pub runs: Vec<Run>,
    pub wrap_width: Option<f32>,
}

/// A scene with every item kind, used by the tests and by `debug_scene` to check the
/// Typst renderer without either front end.
pub fn fixture() -> Scene {
    let black = || Paint::solid("#000");
    let mut s = Scene::new(360.0, 260.0);

    s.push_rect(
        10.0,
        10.0,
        100.0,
        50.0,
        8.0,
        Style::new().fill(Paint::solid("#dae8fc")).stroke(Stroke::new(Paint::solid("#6c8ebf"), 1.0)),
    );
    s.push_text(TextBox::new(10.0, 10.0, 100.0, 50.0, vec![Run::new("Rounded ").size(12.0), Run::new("rect").bold()]));

    s.push_ellipse(
        130.0,
        10.0,
        100.0,
        50.0,
        Style::new()
            .fill(Paint::solid("#fff2cc").gradient_to("#ffd966", 90.0))
            .stroke(Stroke::new(Paint::solid("#d6b656"), 2.0).dash(vec![6.0, 3.0])),
    );
    s.push_polygon(
        &[(250.0, 60.0), (300.0, 10.0), (350.0, 60.0)],
        Style::new().fill(Paint::solid("#d5e8d4")).stroke(Stroke::new(Paint::solid("#82b366"), 1.5).join(Join::Round)),
    );

    let mut p = PathBuilder::new();
    p.move_to(10.0, 120.0).cubic_to(40.0, 70.0, 80.0, 170.0, 110.0, 120.0).line_to(110.0, 150.0).quad_to(60.0, 180.0, 10.0, 150.0).close();
    s.push_path(
        p,
        Style::new().fill(Paint::rgba(0xf8, 0xce, 0xcc, 0xff)).stroke(Stroke::new(Paint::solid("#b85450"), 1.0)),
    );

    s.push_rect(
        140.0,
        100.0,
        80.0,
        40.0,
        0.0,
        Style::new().fill(Paint::solid("#e1d5e7")).stroke(Stroke::new(Paint::solid("#9673a6"), 1.0)).rotation(20.0).opacity(0.6),
    );
    s.push_polyline(&[(240.0, 150.0), (290.0, 110.0), (340.0, 150.0)], Stroke::new(black(), 2.0).cap(Cap::Round).join(Join::Round));

    s.push_text(
        TextBox::new(10.0, 190.0, 160.0, 60.0, vec![
            Run::new("Mixed ").size(11.0),
            Run::new("italic").italic().color("#c00"),
            Run::new(" underline").underline(),
            Run::br(),
            Run::new("struck").strike().size(9.0),
            Run::new(" link").link("https://typst.app").color("#0645ad"),
            Run::new(" and a long tail that has to wrap onto another line").size(10.0),
        ])
        .align(HAlign::Left, VAlign::Top)
        .background(Paint::solid("#f5f5f5")),
    );
    s.push_text(
        TextBox::new(200.0, 190.0, 100.0, 40.0, vec![Run::new("Right bottom, no wrap").font("Courier New")])
            .align(HAlign::Right, VAlign::Bottom)
            .wrap(false),
    );
    s.push_text(TextBox::new(310.0, 160.0, 40.0, 90.0, vec![Run::new("Rotated")]).rotation(-90.0));

    s.warn("unsupported shape \"mxgraph.fake\" drawn as a box");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cbor_round_trip() {
        let scene = fixture();
        let bytes = scene.to_cbor().unwrap();
        assert_eq!(Scene::from_cbor(&bytes).unwrap(), scene);
    }

    #[test]
    fn fixture_has_every_item_kind() {
        let scene = fixture();
        let mut kinds = std::collections::BTreeSet::new();
        for item in &scene.items {
            kinds.insert(match item {
                Item::Shape(s) => match s.geom {
                    Geom::Rect { .. } => "rect",
                    Geom::Ellipse { .. } => "ellipse",
                    Geom::Polygon { .. } => "polygon",
                    Geom::Path { .. } => "path",
                },
                Item::Text(_) => "text",
            });
        }
        assert_eq!(kinds.len(), 5);
        assert_eq!(scene.warnings.len(), 1);
    }

    #[test]
    fn hex_normalizes() {
        assert_eq!(hex("#F00"), "#ff0000ff");
        assert_eq!(hex("336699"), "#336699ff");
        assert_eq!(hex("#33669980"), "#33669980");
        assert_eq!(hex("red"), "#000000ff");
        assert_eq!(hex("#GGGGGGGG"), "#000000ff");
    }

    #[test]
    fn translate_moves_everything() {
        let mut s = Scene::new(10.0, 10.0);
        s.push_rect(1.0, 2.0, 3.0, 4.0, 0.0, Style::new());
        let mut p = PathBuilder::new();
        p.move_to(0.0, 0.0).cubic_to(1.0, 1.0, 2.0, 2.0, 3.0, 3.0);
        s.push_path(p, Style::new());
        s.push_text(TextBox::new(0.0, 0.0, 1.0, 1.0, vec![]));
        s.translate(5.0, 7.0);
        let Item::Shape(sh) = &s.items[0] else { panic!() };
        assert_eq!(sh.bbox, [6.0, 9.0, 3.0, 4.0]);
        let Item::Shape(sh) = &s.items[1] else { panic!() };
        assert_eq!(sh.bbox[0], 5.0);
        let Item::Text(t) = &s.items[2] else { panic!() };
        assert_eq!((t.x, t.y), (5.0, 7.0));
    }

    fn dirty_paint(color: &str) -> Paint {
        Paint { color: color.into(), gradient: None }
    }

    #[test]
    fn sanitizer_drops_non_finite_geometry() {
        let mut s = Scene::new(f32::NAN, f32::INFINITY);
        s.push_rect(0.0, 0.0, f32::NAN, 1.0, 0.0, Style::new());
        s.push_rect(0.0, 0.0, 5.0, 1.0, 0.0, Style::new());
        s.push_polygon(&[(0.0, 0.0), (f32::INFINITY, 1.0)], Style::new());
        let mut p = PathBuilder::new();
        p.move_to(0.0, 0.0).line_to(f32::NEG_INFINITY, 1.0);
        s.push_path(p, Style::new());
        s.push_text(TextBox::new(f32::NAN, 0.0, 1.0, 1.0, vec![]));
        s.push_text(TextBox::new(0.0, 0.0, 1.0, 1.0, vec![]));
        s.sanitize();
        assert_eq!(s.size, [0.0, 0.0]);
        assert_eq!(s.items.len(), 2);
    }

    #[test]
    fn sanitizer_repairs_paints_and_numbers() {
        let mut s = Scene::new(10.0, 10.0);
        let stroke = Stroke::new(dirty_paint("#112233ff"), 1.0).dash(vec![f32::INFINITY, 2.0]);
        let mut style = Style::new().fill(dirty_paint("#GGGGGGGG")).stroke(stroke);
        style.opacity = f32::NAN;
        style.rotation = f32::INFINITY;
        s.push_rect(0.0, 0.0, 5.0, 5.0, 0.0, style);
        s.push_rect(
            0.0,
            0.0,
            5.0,
            5.0,
            0.0,
            Style::new()
                .fill(Paint { color: "#00ff00ff".into(), gradient: Some(Gradient { color: "red".into(), angle: 1.0 }) })
                .stroke(Stroke::new(dirty_paint("zzz"), 1.0)),
        );
        let mut run = Run::new("x").size(f32::NAN);
        run.color = "#12".into();
        s.push_text(TextBox::new(0.0, 0.0, 1.0, 1.0, vec![run]).background(dirty_paint("nope")).rotation(f32::NAN));
        let bytes = s.to_cbor().unwrap();
        let s = Scene::from_cbor(&bytes).unwrap();
        let Item::Shape(a) = &s.items[0] else { panic!() };
        assert!(a.fill.is_none());
        assert_eq!(a.opacity, 1.0);
        assert_eq!(a.rotation, 0.0);
        assert!(a.stroke.as_ref().unwrap().dash.is_empty());
        let Item::Shape(b) = &s.items[1] else { panic!() };
        assert!(b.fill.as_ref().unwrap().gradient.is_none());
        assert!(b.stroke.is_none());
        let Item::Text(t) = &s.items[2] else { panic!() };
        assert!(t.background.is_none());
        assert_eq!(t.rotation, 0.0);
        assert_eq!(t.runs[0].size, 12.0);
        assert_eq!(t.runs[0].color, "#000000ff");
    }

    #[test]
    fn shape_rect_matches_push_rect() {
        let mut s = Scene::new(1.0, 1.0);
        s.push_rect(1.0, 2.0, 3.0, 4.0, 5.0, Style::new());
        assert_eq!(s.items[0], Item::Shape(Shape::rect(1.0, 2.0, 3.0, 4.0, 5.0, Style::new())));
    }

    #[test]
    fn path_bbox_covers_control_points() {
        let mut s = Scene::new(10.0, 10.0);
        let mut p = PathBuilder::new();
        p.move_to(1.0, 2.0).cubic_to(0.0, 5.0, 9.0, 8.0, 4.0, 3.0);
        s.push_path(p, Style::new());
        let Item::Shape(shape) = &s.items[0] else { panic!() };
        assert_eq!(shape.bbox, [0.0, 2.0, 9.0, 6.0]);
    }

    #[test]
    fn warnings_are_deduplicated() {
        let mut s = Scene::new(1.0, 1.0);
        s.warn("a");
        s.warn("a");
        assert_eq!(s.warnings.len(), 1);
    }
}
