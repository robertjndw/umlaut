//! Color names and color syntax as PlantUML accepts it. Every module that reads a
//! color from the source goes through here, so `#red`, `LightBlue` and `#ff0` mean the
//! same thing everywhere.

use crate::color::named_hex;
use crate::scene::{hex, Paint};

/// `#rrggbbaa` for `#f00`, `#ff0000`, `red`, `#red`, `Transparent`. `None` when the text
/// is not a color, so callers can tell a color apart from other words.
pub fn parse_color(s: &str) -> Option<String> {
    let t = s.trim().trim_start_matches('#').trim();
    if t.is_empty() {
        return None;
    }
    let lower = t.to_ascii_lowercase();
    if lower == "transparent" {
        return Some("#00000000".into());
    }
    if matches!(lower.len(), 3 | 6 | 8) && lower.chars().all(|c| c.is_ascii_hexdigit()) {
        return Some(hex(&lower));
    }
    named_hex(&lower)
}

/// Like `parse_color`, and also the two-color gradient forms `#red/blue`, `#red|blue`,
/// `#red-blue` and `#red\blue`.
pub fn parse_paint(s: &str) -> Option<Paint> {
    if let Some(c) = parse_color(s) {
        return Some(Paint::solid(&c));
    }
    let t = s.trim().trim_start_matches('#');
    for (sep, angle) in [('|', 0.0), ('-', 90.0), ('\\', 45.0), ('/', 315.0)] {
        if let Some((a, b)) = t.split_once(sep) {
            if let (Some(a), Some(b)) = (parse_color(a), parse_color(b)) {
                return Some(Paint::solid(&a).gradient_to(&b, angle));
            }
        }
    }
    None
}

/// Gray with the same perceived brightness; `skinparam monochrome true` maps every
/// color through this.
pub fn to_gray(color: &str) -> String {
    let c = hex(color);
    let v = |i: usize| u32::from_str_radix(&c[i..i + 2], 16).unwrap_or(0) as f32;
    let y = (0.299 * v(1) + 0.587 * v(3) + 0.114 * v(5)).round() as u32;
    format!("#{y:02x}{y:02x}{y:02x}{}", &c[7..9])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors() {
        assert_eq!(parse_color("#red").unwrap(), "#ff0000ff");
        assert_eq!(parse_color("LightBlue").unwrap(), "#add8e6ff");
        assert_eq!(parse_color("#E2E2F0").unwrap(), "#e2e2f0ff");
        assert_eq!(parse_color("#fa0").unwrap(), "#ffaa00ff");
        assert_eq!(parse_color("transparent").unwrap(), "#00000000");
        assert!(parse_color("notacolor").is_none());
        assert!(parse_color("Pale Green").is_some());
    }

    #[test]
    fn gradients() {
        let p = parse_paint("#white/lightblue").unwrap();
        assert_eq!(p.gradient.unwrap().color, "#add8e6ff");
        assert!(parse_paint("#red|").is_none());
    }

    #[test]
    fn gray() {
        assert_eq!(to_gray("#ffffff"), "#ffffffff");
        assert_eq!(to_gray("#000000"), "#000000ff");
    }
}
