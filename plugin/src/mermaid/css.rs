//! The CSS-like styles Mermaid diagrams share: `style id fill:#f9f,stroke:#333`,
//! `classDef name ...`, `class a,b name` and `id:::name`. Only what the scene can draw is
//! kept (fill, stroke, stroke width, dashes, text color, bold, italic); other properties
//! are dropped without a warning, as most of them only make sense in a browser.

use std::collections::HashMap;

use crate::color::parse_css;
use crate::plantuml::diagrams::gcommon::Look;
use crate::scene::Paint;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Css {
    pub fill: Option<String>,
    pub stroke: Option<String>,
    pub stroke_width: Option<f32>,
    pub dash: Option<Vec<f32>>,
    pub color: Option<String>,
    pub bold: bool,
    pub italic: bool,
}

fn px(v: &str) -> Option<f32> {
    v.trim().trim_end_matches("px").trim().parse().ok().filter(|n: &f32| n.is_finite() && *n >= 0.0)
}

/// Splits `a:b,c:rgb(1,2,3),stroke-dasharray:5,5` into declarations. Commas inside
/// parentheses belong to the value, and a piece without a `:` continues the previous
/// value (`stroke-dasharray: 5, 5`).
fn declarations(s: &str) -> Vec<(String, String)> {
    let mut pieces = Vec::new();
    let (mut depth, mut start) = (0i32, 0);
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth <= 0 => {
                pieces.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    pieces.push(&s[start..]);
    let mut out: Vec<(String, String)> = Vec::new();
    for p in pieces {
        match p.split_once(':') {
            Some((k, v)) => out.push((k.trim().to_ascii_lowercase(), v.trim().to_string())),
            None => {
                if let Some(last) = out.last_mut() {
                    last.1.push(',');
                    last.1.push_str(p.trim());
                }
            }
        }
    }
    out
}

impl Css {
    /// Parses the declarations of a `style` or `classDef` statement.
    pub fn parse(s: &str) -> Css {
        let mut css = Css::default();
        for (k, v) in declarations(s.trim().trim_end_matches(';')) {
            let v = v.trim_end_matches("!important").trim();
            match k.as_str() {
                "fill" | "background" | "background-color" => css.fill = parse_css(v),
                "stroke" | "border-color" => css.stroke = parse_css(v),
                "stroke-width" => css.stroke_width = px(v),
                "stroke-dasharray" => {
                    let d: Vec<f32> = v.split([' ', ',']).filter_map(px).collect();
                    css.dash = if d.is_empty() || d.iter().all(|x| *x == 0.0) { None } else { Some(d) };
                }
                "color" => css.color = parse_css(v),
                "font-weight" => css.bold = v.eq_ignore_ascii_case("bold") || v.parse::<u32>().is_ok_and(|w| w >= 600),
                "font-style" => css.italic = v.eq_ignore_ascii_case("italic") || v.eq_ignore_ascii_case("oblique"),
                _ => {}
            }
        }
        css
    }

    /// `other` on top of `self`: every property `other` sets wins.
    pub fn merge(&mut self, other: &Css) {
        if other.fill.is_some() {
            self.fill.clone_from(&other.fill);
        }
        if other.stroke.is_some() {
            self.stroke.clone_from(&other.stroke);
        }
        if other.stroke_width.is_some() {
            self.stroke_width = other.stroke_width;
        }
        if other.dash.is_some() {
            self.dash.clone_from(&other.dash);
        }
        if other.color.is_some() {
            self.color.clone_from(&other.color);
        }
        self.bold |= other.bold;
        self.italic |= other.italic;
    }

    /// The same style in the shape PlantUML's graph layouts take. Look has no stroke
    /// width, so anything from 2 up is drawn bold.
    pub fn look(&self) -> Look {
        Look {
            fill: self.fill.as_deref().map(Paint::solid),
            line: self.stroke.clone(),
            text: self.color.clone(),
            dashed: self.dash.is_some(),
            bold: self.stroke_width.is_some_and(|w| w >= 2.0),
        }
    }
}

/// The `classDef`s of a diagram. `default` applies to every node before its own classes.
#[derive(Debug, Default)]
pub struct Classes {
    defs: HashMap<String, Css>,
}

impl Classes {
    /// `classDef a,b fill:...`: the rest after the keyword.
    pub fn define(&mut self, rest: &str) {
        let rest = rest.trim();
        let (names, decl) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        let css = Css::parse(decl);
        for n in names.split(',').map(str::trim).filter(|n| !n.is_empty()) {
            self.defs.entry(n.to_string()).or_default().merge(&css);
        }
    }

    /// `default`, then each class in order, then the element's own `style`.
    pub fn resolve(&self, classes: &[String], own: Option<&Css>) -> Css {
        let mut css = self.defs.get("default").cloned().unwrap_or_default();
        for c in classes {
            if let Some(d) = self.defs.get(c) {
                css.merge(d);
            }
        }
        if let Some(o) = own {
            css.merge(o);
        }
        css
    }
}

/// `style id fill:...`: the rest after the keyword, as `(id, css)`.
pub fn parse_style(rest: &str) -> Option<(String, Css)> {
    let rest = rest.trim();
    let (id, decl) = rest.split_once(char::is_whitespace)?;
    Some((id.trim().to_string(), Css::parse(decl)))
}

/// `class a,b name` (flowchart and state) or `cssClass "a,b" name` (class diagrams): the
/// rest after the keyword, as `(ids, class names)`.
pub fn parse_assign(rest: &str) -> Option<(Vec<String>, Vec<String>)> {
    let rest = rest.trim();
    let (ids, names) = rest.rsplit_once(char::is_whitespace)?;
    let ids: Vec<String> = ids.trim().trim_matches('"').split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    let names: Vec<String> = names.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    (!ids.is_empty() && !names.is_empty()).then_some((ids, names))
}

/// Splits `id:::a:::b` into the id and its classes. The id is left as written, quotes
/// and shape brackets included.
pub fn split_classes(s: &str) -> (&str, Vec<String>) {
    let mut parts = s.split(":::");
    let head = parts.next().unwrap_or("");
    (head, parts.map(|c| c.trim().to_string()).filter(|c| !c.is_empty()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_declarations() {
        let c = Css::parse("fill:#f9f,stroke:rgb(51, 51, 51),stroke-width:4px,color:white,stroke-dasharray: 5, 5,font-weight:bold");
        assert_eq!(c.fill.as_deref(), Some("#ff99ffff"));
        assert_eq!(c.stroke.as_deref(), Some("#333333ff"));
        assert_eq!(c.stroke_width, Some(4.0));
        assert_eq!(c.color.as_deref(), Some("#ffffffff"));
        assert_eq!(c.dash, Some(vec![5.0, 5.0]));
        assert!(c.bold && !c.italic);
        assert!(c.look().bold && c.look().dashed);
    }

    #[test]
    fn ignores_what_cannot_be_drawn() {
        let c = Css::parse("font-size:20px,rx:4,fill:nonsense;");
        assert_eq!(c, Css::default());
    }

    #[test]
    fn classes_resolve_in_order() {
        let mut cl = Classes::default();
        cl.define("default fill:#eee,stroke:#000");
        cl.define("a,b fill:#f00");
        cl.define("b stroke:#00f");
        let own = Css::parse("stroke-width:3");
        let c = cl.resolve(&["a".into(), "b".into()], Some(&own));
        assert_eq!(c.fill.as_deref(), Some("#ff0000ff"));
        assert_eq!(c.stroke.as_deref(), Some("#0000ffff"));
        assert_eq!(c.stroke_width, Some(3.0));
    }

    #[test]
    fn statements() {
        assert_eq!(parse_style("A fill:#fff").unwrap().0, "A");
        assert_eq!(parse_assign("a,b warn").unwrap(), (vec!["a".to_string(), "b".to_string()], vec!["warn".to_string()]));
        assert_eq!(parse_assign("\"a,b\" warn").unwrap().0, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(split_classes("A[x]:::one:::two"), ("A[x]", vec!["one".to_string(), "two".to_string()]));
    }
}
