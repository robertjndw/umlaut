//! draw.io style strings: `key=value;` pairs plus bare tokens that name a base style
//! (`ellipse;`, `text;`, `swimlane;`).

use std::collections::HashMap;

use crate::color;
use crate::scene::hex;

/// What draw.io's default stylesheet puts behind the bare tokens. Only the keys that
/// change how we draw are listed.
const BASE_STYLES: &[(&str, &str)] = &[
    ("ellipse", "shape=ellipse;perimeter=ellipsePerimeter"),
    ("rhombus", "shape=rhombus;perimeter=rhombusPerimeter"),
    ("triangle", "shape=triangle;perimeter=trianglePerimeter"),
    ("hexagon", "shape=hexagon;perimeter=hexagonPerimeter2"),
    ("cloud", "shape=cloud;perimeter=ellipsePerimeter"),
    ("cylinder", "shape=cylinder"),
    ("actor", "shape=actor"),
    ("doubleEllipse", "shape=doubleEllipse;perimeter=ellipsePerimeter"),
    ("line", "shape=line"),
    ("image", "shape=image;verticalLabelPosition=bottom;verticalAlign=top;labelBackgroundColor=default"),
    ("label", "shape=label"),
    ("text", "shape=text;fillColor=none;strokeColor=none"),
    ("swimlane", "shape=swimlane;fontStyle=1;startSize=23;verticalAlign=top"),
    ("group", "fillColor=none;strokeColor=none"),
    ("edgeLabel", "fillColor=none;strokeColor=none;labelBackgroundColor=default;fontSize=11"),
];

#[derive(Debug, Clone, Default)]
pub struct Style {
    map: HashMap<String, String>,
}

impl Style {
    pub fn parse(source: &str) -> Style {
        let mut style = Style::default();
        for part in source.split(';') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            match part.split_once('=') {
                Some((k, v)) => {
                    style.map.insert(k.trim().to_string(), v.trim().to_string());
                }
                // Named styles are merged in place, so a later key=value still wins
                // over the base style and the other way around.
                None => {
                    if let Some((_, defaults)) = BASE_STYLES.iter().find(|(name, _)| *name == part) {
                        for pair in defaults.split(';') {
                            if let Some((k, v)) = pair.split_once('=') {
                                style.map.insert(k.to_string(), v.to_string());
                            }
                        }
                    }
                }
            }
        }
        style
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.map.get(key).map(|s| s.as_str())
    }

    pub fn str_or<'a>(&'a self, key: &str, default: &'a str) -> &'a str {
        self.get(key).filter(|v| !v.is_empty()).unwrap_or(default)
    }

    pub fn num(&self, key: &str, default: f32) -> f32 {
        self.get(key).and_then(|v| v.trim().parse::<f32>().ok()).filter(|v| v.is_finite()).unwrap_or(default)
    }

    pub fn opt_num(&self, key: &str) -> Option<f32> {
        self.get(key).and_then(|v| v.trim().parse::<f32>().ok()).filter(|v| v.is_finite())
    }

    pub fn flag(&self, key: &str, default: bool) -> bool {
        match self.get(key) {
            Some("1") | Some("true") => true,
            Some("0") | Some("false") => false,
            _ => default,
        }
    }

    pub fn has(&self, key: &str) -> bool {
        self.map.contains_key(key)
    }

    /// A color as `#rrggbbaa`. `none` gives `None`; a missing or `default` value gives
    /// `default` (which may itself be `None`).
    pub fn color(&self, key: &str, default: Option<&str>) -> Option<String> {
        match self.get(key) {
            None => default.map(hex),
            Some(v) => parse_color(v, default),
        }
    }

    pub fn shape(&self) -> Option<&str> {
        self.get("shape").filter(|s| !s.is_empty())
    }
}

pub fn parse_color(value: &str, default: Option<&str>) -> Option<String> {
    let v = value.trim();
    // `light-dark(a, b)`: the page is always light here.
    let v = match v.strip_prefix("light-dark(").and_then(|r| r.strip_suffix(')')) {
        Some(inner) => split_top_level_comma(inner).0.trim(),
        None => v,
    };
    match v {
        "" | "default" | "inherit" | "swimlane" | "indicated" => default.map(hex),
        "none" => None,
        "transparent" => Some("#ffffff00".to_string()),
        _ => color::named_hex(v)
            .or_else(|| color::normalize(v))
            .or_else(|| default.map(hex)),
    }
}

fn split_top_level_comma(s: &str) -> (&str, &str) {
    let mut depth = 0;
    for (i, ch) in s.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => return (&s[..i], &s[i + 1..]),
            _ => {}
        }
    }
    (s, "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pairs_and_tokens() {
        let s = Style::parse("ellipse;whiteSpace=wrap;html=1;fillColor=#dae8fc;strokeColor=#6c8ebf;");
        assert_eq!(s.shape(), Some("ellipse"));
        assert_eq!(s.get("whiteSpace"), Some("wrap"));
        assert!(s.flag("html", false));
        assert_eq!(s.color("fillColor", None).as_deref(), Some("#dae8fcff"));
    }

    #[test]
    fn later_pairs_override_base_style() {
        let s = Style::parse("text;fillColor=#ff0000");
        assert_eq!(s.color("fillColor", Some("#ffffff")).as_deref(), Some("#ff0000ff"));
        assert_eq!(s.color("strokeColor", Some("#000000")), None);
        assert_eq!(s.shape(), Some("text"));
    }

    #[test]
    fn colors() {
        let s = Style::parse("a=none;b=default;c=#fff;d=light-dark(#111111,#eeeeee);e=white;f=bogus");
        assert_eq!(s.color("a", Some("#000000")), None);
        assert_eq!(s.color("b", Some("#123456")).as_deref(), Some("#123456ff"));
        assert_eq!(s.color("c", None).as_deref(), Some("#ffffffff"));
        assert_eq!(s.color("d", None).as_deref(), Some("#111111ff"));
        assert_eq!(s.color("e", None).as_deref(), Some("#ffffffff"));
        assert_eq!(s.color("f", Some("#000000")).as_deref(), Some("#000000ff"));
        assert_eq!(s.color("missing", Some("#abc")).as_deref(), Some("#aabbccff"));
    }

    #[test]
    fn invalid_colors_fall_back_to_the_default() {
        let s = Style::parse("a=#GGGGGGGG;b=#12345;c=cornflowerblue;d=#zzz");
        assert_eq!(s.color("a", Some("#000000")).as_deref(), Some("#000000ff"));
        assert_eq!(s.color("a", None), None);
        assert_eq!(s.color("b", None), None);
        assert_eq!(s.color("d", None), None);
        assert_eq!(s.color("c", None).as_deref(), Some("#6495edff"));
    }

    #[test]
    fn numbers_and_flags() {
        let s = Style::parse("fontSize=14;rounded=1;dashed=0;strokeWidth=abc");
        assert_eq!(s.num("fontSize", 12.0), 14.0);
        assert_eq!(s.num("strokeWidth", 1.0), 1.0);
        assert!(s.flag("rounded", false));
        assert!(!s.flag("dashed", true));
        assert!(s.flag("missing", true));
    }

    #[test]
    fn values_may_contain_equals() {
        let s = Style::parse("image=data:image/png,base64=abc;x=1");
        assert_eq!(s.get("image"), Some("data:image/png,base64=abc"));
    }
}
