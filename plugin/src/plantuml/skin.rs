//! The look of a diagram: PlantUML's default skin plus the `skinparam` subset we honor.
//!
//! Skinparams are kept as a flat map with normalized keys (lowercase, letters and
//! digits only). Block form `skinparam sequence { ArrowColor red }` is flattened to
//! `sequencearrowcolor` before it gets here. Diagram code asks for values by a list of
//! key prefixes, most specific first, e.g. `["sequenceparticipant", "participant"]`
//! looks up `sequenceparticipantbackgroundcolor`, then `participantbackgroundcolor`,
//! then returns the caller's default. Unknown skinparams are simply never asked for.
//!
//! Font sizes: runs always carry an explicit size taken from PlantUML's defaults (14
//! for most text, 13 for messages and notes, 12 for activities) or from the skinparams.
//! The document's `text.size` is deliberately not used, so a diagram looks like it
//! does on plantuml.com whatever the surrounding text is, and `width`/`scale` on the
//! Typst side resize the whole picture. `skinparam defaultFontSize` overrides every
//! default size at once.

use std::collections::HashMap;

use super::colors::{parse_color, parse_paint, to_gray};
use super::creole::Base;
use crate::scene::{hex, Paint};

pub const BORDER: &str = "#181818";
pub const BLACK: &str = "#000000";
pub const PARTICIPANT_FILL: &str = "#E2E2F0";
pub const NOTE_FILL: &str = "#FEFFDD";
pub const ACTIVITY_FILL: &str = "#F1F1F1";
pub const GROUP_TAB_FILL: &str = "#EEEEEE";
pub const WHITE: &str = "#FFFFFF";

fn norm(key: &str) -> String {
    key.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase()
}

#[derive(Debug, Clone, Default)]
pub struct Skin {
    params: HashMap<String, String>,
}

impl Skin {
    pub fn new() -> Skin {
        Skin::default()
    }

    /// Stores a skinparam. Returns a warning for settings we know and cannot honor.
    pub fn set(&mut self, key: &str, value: &str) -> Option<String> {
        let k = norm(key);
        let v = value.trim().trim_matches('"').trim().to_string();
        let warning = match k.as_str() {
            "handwritten" if v.eq_ignore_ascii_case("true") => Some("skinparam handwritten is not supported and is ignored".to_string()),
            "linetype" if !matches!(v.to_ascii_lowercase().as_str(), "polyline" | "ortho" | "spline") => Some(format!("skinparam linetype {v} is not supported and is ignored")),
            _ => None,
        };
        self.params.insert(k, v);
        warning
    }

    pub fn raw(&self, key: &str) -> Option<&str> {
        // Most diagrams set no skinparams; skip the key allocation for them.
        if self.params.is_empty() {
            return None;
        }
        self.params.get(&norm(key)).map(|s| s.as_str())
    }

    pub fn mono(&self) -> bool {
        self.flag("monochrome").unwrap_or(false)
    }

    pub fn flag(&self, key: &str) -> Option<bool> {
        match self.raw(key)?.to_ascii_lowercase().as_str() {
            "true" | "on" | "yes" => Some(true),
            "false" | "off" | "no" => Some(false),
            _ => None,
        }
    }

    fn first(&self, prefixes: &[&str], suffix: &str) -> Option<&str> {
        if self.params.is_empty() {
            return None;
        }
        prefixes.iter().find_map(|p| self.params.get(&norm(&format!("{p}{suffix}")))).map(|s| s.as_str())
    }

    fn tint(&self, c: String) -> String {
        if self.mono() { to_gray(&c) } else { c }
    }

    /// Color for exact keys (first match wins) as `#rrggbbaa`.
    pub fn color(&self, keys: &[&str], default: &str) -> String {
        let found = keys.iter().find_map(|k| self.raw(k).and_then(parse_color));
        self.tint(found.unwrap_or_else(|| hex(default)))
    }

    /// Fill for `<prefix>BackgroundColor`; gradients are allowed.
    pub fn fill(&self, prefixes: &[&str], default: &str) -> Paint {
        let found = self.first(prefixes, "backgroundcolor").and_then(parse_paint);
        let mut p = found.unwrap_or_else(|| Paint::solid(default));
        if self.mono() {
            p.color = to_gray(&p.color);
            if let Some(g) = p.gradient.as_mut() {
                g.color = to_gray(&g.color);
            }
        }
        p
    }

    /// `<prefix>BorderColor`.
    pub fn border(&self, prefixes: &[&str], default: &str) -> String {
        let found = self.first(prefixes, "bordercolor").and_then(parse_color);
        self.tint(found.unwrap_or_else(|| hex(default)))
    }

    pub fn num(&self, keys: &[&str], default: f32) -> f32 {
        keys.iter().find_map(|k| self.raw(k).and_then(|v| v.trim_end_matches("px").parse::<f32>().ok())).unwrap_or(default)
    }

    pub fn default_font_size(&self) -> Option<f32> {
        self.raw("defaultfontsize").and_then(|v| v.parse().ok())
    }

    /// Text style for `<prefix>FontSize/FontColor/FontName/FontStyle`, falling back to
    /// the `default*` skinparams and then to the given defaults.
    pub fn text(&self, prefixes: &[&str], default_size: f32, default_color: &str) -> Base {
        let size = self
            .first(prefixes, "fontsize")
            .and_then(|v| v.parse::<f32>().ok())
            .or_else(|| self.default_font_size())
            .unwrap_or(default_size);
        let color = self
            .first(prefixes, "fontcolor")
            .or_else(|| self.raw("defaultfontcolor"))
            .and_then(parse_color)
            .unwrap_or_else(|| hex(default_color));
        let mut base = Base::new(size, &self.tint(color));
        base.font = self.first(prefixes, "fontname").or_else(|| self.raw("defaultfontname")).map(|s| s.to_string());
        if let Some(style) = self.first(prefixes, "fontstyle") {
            let style = style.to_ascii_lowercase();
            base.bold |= style.contains("bold");
            base.italic |= style.contains("italic");
        }
        base
    }

    /// `roundCorner` is PlantUML's diameter-like value; the drawn radius is half of it.
    pub fn radius(&self, prefixes: &[&str], default_round_corner: f32) -> f32 {
        let own = self.first(prefixes, "roundcorner").and_then(|v| v.parse::<f32>().ok());
        own.or_else(|| self.raw("roundcorner").and_then(|v| v.parse().ok())).unwrap_or(default_round_corner) / 2.0
    }

    /// Whole-diagram background, `None` for transparent (the default).
    pub fn background(&self) -> Option<Paint> {
        let p = self.raw("backgroundcolor").and_then(parse_paint)?;
        if p.color.ends_with("00") && p.gradient.is_none() {
            return None;
        }
        Some(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_prefers_specific_keys() {
        let mut s = Skin::new();
        s.set("ParticipantBackgroundColor", "red");
        s.set("sequence ParticipantBackgroundColor", "blue");
        assert_eq!(s.fill(&["sequenceparticipant", "participant"], PARTICIPANT_FILL).color, "#0000ffff");
        assert_eq!(s.fill(&["participant"], PARTICIPANT_FILL).color, "#ff0000ff");
        assert_eq!(s.fill(&["actor"], PARTICIPANT_FILL).color, hex(PARTICIPANT_FILL));
    }

    #[test]
    fn font_defaults_cascade() {
        let mut s = Skin::new();
        assert_eq!(s.text(&["note"], 13.0, BLACK).size, 13.0);
        s.set("defaultFontSize", "20");
        assert_eq!(s.text(&["note"], 13.0, BLACK).size, 20.0);
        s.set("NoteFontSize", "9");
        assert_eq!(s.text(&["note"], 13.0, BLACK).size, 9.0);
        s.set("defaultFontName", "Courier");
        assert_eq!(s.text(&["note"], 13.0, BLACK).font.as_deref(), Some("Courier"));
    }

    #[test]
    fn monochrome_grays_everything() {
        let mut s = Skin::new();
        s.set("monochrome", "true");
        let f = s.fill(&["x"], "#ff0000").color;
        assert_eq!(&f[1..3], &f[3..5]);
    }

    #[test]
    fn warnings_and_flags() {
        let mut s = Skin::new();
        assert!(s.set("handwritten", "true").is_some());
        assert!(s.set("shadowing", "false").is_none());
        assert_eq!(s.flag("shadowing"), Some(false));
        s.set("roundCorner", "20");
        assert_eq!(s.radius(&["x"], 5.0), 10.0);
        assert!(s.background().is_none());
        s.set("backgroundColor", "white");
        assert!(s.background().is_some());
    }
}
