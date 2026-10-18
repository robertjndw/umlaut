//! Color names and strict hex parsing shared by the draw.io and PlantUML front ends.

/// `#rrggbbaa` for `#rgb`, `#rrggbb` and `#rrggbbaa` (with or without the `#`), `None`
/// for anything else. Strict on purpose: the renderer hands this string to Typst's
/// `rgb()`, which fails the whole document on a bad one.
pub fn normalize(color: &str) -> Option<String> {
    let c = color.trim().trim_start_matches('#').to_ascii_lowercase();
    if !c.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    match c.len() {
        3 => Some(format!("#{}ff", c.chars().flat_map(|ch| [ch, ch]).collect::<String>())),
        6 => Some(format!("#{c}ff")),
        8 => Some(format!("#{c}")),
        _ => None,
    }
}

/// Whether `s` is exactly what `normalize` produces, which is all the renderer accepts.
pub fn is_valid_output(s: &str) -> bool {
    s.len() == 9 && s.starts_with('#') && s[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

/// The HTML/CSS color names (which are also the X11 names PlantUML's base table is
/// built from). Gray and grey spellings are both listed.
const NAMES: &[(&str, u32)] = &[
    ("aliceblue", 0xf0f8ff), ("antiquewhite", 0xfaebd7), ("aqua", 0x00ffff), ("aquamarine", 0x7fffd4),
    ("azure", 0xf0ffff), ("beige", 0xf5f5dc), ("bisque", 0xffe4c4), ("black", 0x000000),
    ("blanchedalmond", 0xffebcd), ("blue", 0x0000ff), ("blueviolet", 0x8a2be2), ("brown", 0xa52a2a),
    ("burlywood", 0xdeb887), ("cadetblue", 0x5f9ea0), ("chartreuse", 0x7fff00), ("chocolate", 0xd2691e),
    ("coral", 0xff7f50), ("cornflowerblue", 0x6495ed), ("cornsilk", 0xfff8dc), ("crimson", 0xdc143c),
    ("cyan", 0x00ffff), ("darkblue", 0x00008b), ("darkcyan", 0x008b8b), ("darkgoldenrod", 0xb8860b),
    ("darkgray", 0xa9a9a9), ("darkgrey", 0xa9a9a9), ("darkgreen", 0x006400), ("darkkhaki", 0xbdb76b),
    ("darkmagenta", 0x8b008b), ("darkolivegreen", 0x556b2f), ("darkorange", 0xff8c00), ("darkorchid", 0x9932cc),
    ("darkred", 0x8b0000), ("darksalmon", 0xe9967a), ("darkseagreen", 0x8fbc8f), ("darkslateblue", 0x483d8b),
    ("darkslategray", 0x2f4f4f), ("darkslategrey", 0x2f4f4f), ("darkturquoise", 0x00ced1), ("darkviolet", 0x9400d3),
    ("deeppink", 0xff1493), ("deepskyblue", 0x00bfff), ("dimgray", 0x696969), ("dimgrey", 0x696969),
    ("dodgerblue", 0x1e90ff), ("firebrick", 0xb22222), ("floralwhite", 0xfffaf0), ("forestgreen", 0x228b22),
    ("fuchsia", 0xff00ff), ("gainsboro", 0xdcdcdc), ("ghostwhite", 0xf8f8ff), ("gold", 0xffd700),
    ("goldenrod", 0xdaa520), ("gray", 0x808080), ("grey", 0x808080), ("green", 0x008000),
    ("greenyellow", 0xadff2f), ("honeydew", 0xf0fff0), ("hotpink", 0xff69b4), ("indianred", 0xcd5c5c),
    ("indigo", 0x4b0082), ("ivory", 0xfffff0), ("khaki", 0xf0e68c), ("lavender", 0xe6e6fa),
    ("lavenderblush", 0xfff0f5), ("lawngreen", 0x7cfc00), ("lemonchiffon", 0xfffacd), ("lightblue", 0xadd8e6),
    ("lightcoral", 0xf08080), ("lightcyan", 0xe0ffff), ("lightgoldenrodyellow", 0xfafad2), ("lightgray", 0xd3d3d3),
    ("lightgrey", 0xd3d3d3), ("lightgreen", 0x90ee90), ("lightpink", 0xffb6c1), ("lightsalmon", 0xffa07a),
    ("lightseagreen", 0x20b2aa), ("lightskyblue", 0x87cefa), ("lightslategray", 0x778899), ("lightslategrey", 0x778899),
    ("lightsteelblue", 0xb0c4de), ("lightyellow", 0xffffe0), ("lime", 0x00ff00), ("limegreen", 0x32cd32),
    ("linen", 0xfaf0e6), ("magenta", 0xff00ff), ("maroon", 0x800000), ("mediumaquamarine", 0x66cdaa),
    ("mediumblue", 0x0000cd), ("mediumorchid", 0xba55d3), ("mediumpurple", 0x9370db), ("mediumseagreen", 0x3cb371),
    ("mediumslateblue", 0x7b68ee), ("mediumspringgreen", 0x00fa9a), ("mediumturquoise", 0x48d1cc), ("mediumvioletred", 0xc71585),
    ("midnightblue", 0x191970), ("mintcream", 0xf5fffa), ("mistyrose", 0xffe4e1), ("moccasin", 0xffe4b5),
    ("navajowhite", 0xffdead), ("navy", 0x000080), ("oldlace", 0xfdf5e6), ("olive", 0x808000),
    ("olivedrab", 0x6b8e23), ("orange", 0xffa500), ("orangered", 0xff4500), ("orchid", 0xda70d6),
    ("palegoldenrod", 0xeee8aa), ("palegreen", 0x98fb98), ("paleturquoise", 0xafeeee), ("palevioletred", 0xdb7093),
    ("papayawhip", 0xffefd5), ("peachpuff", 0xffdab9), ("peru", 0xcd853f), ("pink", 0xffc0cb),
    ("plum", 0xdda0dd), ("powderblue", 0xb0e0e6), ("purple", 0x800080), ("rebeccapurple", 0x663399),
    ("red", 0xff0000), ("rosybrown", 0xbc8f8f), ("royalblue", 0x4169e1), ("saddlebrown", 0x8b4513),
    ("salmon", 0xfa8072), ("sandybrown", 0xf4a460), ("seagreen", 0x2e8b57), ("seashell", 0xfff5ee),
    ("sienna", 0xa0522d), ("silver", 0xc0c0c0), ("skyblue", 0x87ceeb), ("slateblue", 0x6a5acd),
    ("slategray", 0x708090), ("slategrey", 0x708090), ("snow", 0xfffafa), ("springgreen", 0x00ff7f),
    ("steelblue", 0x4682b4), ("tan", 0xd2b48c), ("teal", 0x008080), ("thistle", 0xd8bfd8),
    ("tomato", 0xff6347), ("turquoise", 0x40e0d0), ("violet", 0xee82ee), ("wheat", 0xf5deb3),
    ("white", 0xffffff), ("whitesmoke", 0xf5f5f5), ("yellow", 0xffff00), ("yellowgreen", 0x9acd32),
];

/// Colors PlantUML adds on top of the HTML set.
const PLANTUML_NAMES: &[(&str, u32)] = &[
    ("application", 0xffffcc), ("business", 0xffffcc), ("implementation", 0xc0ffc0), ("motivation", 0xd1bddb),
    ("physical", 0xc9ffc9), ("strategy", 0xf8e7c0), ("technology", 0xc9ffc9),
];

pub fn named(name: &str) -> Option<u32> {
    let key: String = name.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase();
    NAMES.iter().chain(PLANTUML_NAMES).find(|(n, _)| *n == key).map(|&(_, v)| v)
}

/// `named` as an opaque `#rrggbbaa` color.
pub fn named_hex(name: &str) -> Option<String> {
    named(name).map(|v| format!("#{v:06x}ff"))
}

/// `#rrggbbaa` for a CSS color: hex (3, 4, 6 or 8 digits), `rgb()`/`rgba()`,
/// `hsl()`/`hsla()` in the comma or the space syntax, a CSS color name or `transparent`.
/// `None` for anything else. Unlike `named`, PlantUML's extra names are not accepted.
pub fn parse_css(s: &str) -> Option<String> {
    let t = s.trim().to_ascii_lowercase();
    if let Some(hex) = t.strip_prefix('#') {
        // CSS also has `#rgba`, which `normalize` does not.
        if hex.len() == 4 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Some(format!("#{}", hex.chars().flat_map(|ch| [ch, ch]).collect::<String>()));
        }
        return normalize(&t);
    }
    if t == "transparent" {
        return Some("#00000000".into());
    }
    if let Some((func, args)) = t.strip_suffix(')').and_then(|r| r.split_once('(')) {
        let parts: Vec<&str> = args.split(|c: char| c == ',' || c == '/' || c.is_whitespace()).filter(|p| !p.is_empty()).collect();
        if parts.len() != 3 && parts.len() != 4 {
            return None;
        }
        let alpha = match parts.get(3) {
            Some(a) => unit_interval(a)?,
            None => 1.0,
        };
        let (r, g, b) = match func.trim() {
            "rgb" | "rgba" => {
                let ch = |p: &str| match p.strip_suffix('%') {
                    Some(v) => Some(v.parse::<f32>().ok()? / 100.0 * 255.0),
                    None => p.parse::<f32>().ok(),
                };
                (ch(parts[0])?, ch(parts[1])?, ch(parts[2])?)
            }
            "hsl" | "hsla" => {
                let hue = parts[0].strip_suffix("deg").unwrap_or(parts[0]).parse::<f32>().ok()?;
                let pct = |p: &str| p.strip_suffix('%').unwrap_or(p).parse::<f32>().ok().map(|v| (v / 100.0).clamp(0.0, 1.0));
                hsl_to_rgb(hue, pct(parts[1])?, pct(parts[2])?)
            }
            _ => return None,
        };
        if ![r, g, b].iter().all(|v| v.is_finite()) {
            return None;
        }
        let byte = |v: f32| v.clamp(0.0, 255.0).round() as u8;
        return Some(format!("#{:02x}{:02x}{:02x}{:02x}", byte(r), byte(g), byte(b), byte(alpha * 255.0)));
    }
    NAMES.iter().find(|(n, _)| *n == t).map(|&(_, v)| format!("#{v:06x}ff"))
}

/// An alpha value: a number or a percentage, clamped to 0..=1.
fn unit_interval(p: &str) -> Option<f32> {
    let v = match p.strip_suffix('%') {
        Some(v) => v.parse::<f32>().ok()? / 100.0,
        None => p.parse::<f32>().ok()?,
    };
    v.is_finite().then(|| v.clamp(0.0, 1.0))
}

/// Channels in 0..=255 for a hue in degrees and saturation and lightness in 0..=1.
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    let h = h.rem_euclid(360.0) / 60.0;
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    ((r + m) * 255.0, (g + m) * 255.0, (b + m) * 255.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_strictly() {
        assert_eq!(normalize("#F00").as_deref(), Some("#ff0000ff"));
        assert_eq!(normalize("336699").as_deref(), Some("#336699ff"));
        assert_eq!(normalize("#33669980").as_deref(), Some("#33669980"));
        assert_eq!(normalize("#GGGGGGGG"), None);
        assert_eq!(normalize("#12345"), None);
        assert_eq!(normalize("red"), None);
        assert_eq!(normalize(""), None);
    }

    #[test]
    fn output_validity() {
        assert!(is_valid_output("#00ff00ff"));
        assert!(!is_valid_output("#00ff00"));
        assert!(!is_valid_output("#GGGGGGGG"));
        assert!(!is_valid_output("#\u{e9}\u{e9}0000ff"));
    }

    #[test]
    fn css_hex_forms() {
        assert_eq!(parse_css("#f00").as_deref(), Some("#ff0000ff"));
        assert_eq!(parse_css("#f008").as_deref(), Some("#ff000088"));
        assert_eq!(parse_css("#ECECFF").as_deref(), Some("#ececffff"));
        assert_eq!(parse_css("#11223344").as_deref(), Some("#11223344"));
        assert_eq!(parse_css("#12345"), None);
        assert_eq!(parse_css("336699"), None);
    }

    #[test]
    fn css_rgb_and_hsl() {
        assert_eq!(parse_css("rgb(255, 0, 128)").as_deref(), Some("#ff0080ff"));
        assert_eq!(parse_css("RGB(255 0 128)").as_deref(), Some("#ff0080ff"));
        assert_eq!(parse_css("rgba(0,0,0,0.5)").as_deref(), Some("#00000080"));
        assert_eq!(parse_css("rgb(100%, 0%, 0% / 50%)").as_deref(), Some("#ff000080"));
        assert_eq!(parse_css("rgb(300, -5, 0)").as_deref(), Some("#ff0000ff"));
        assert_eq!(parse_css("hsl(0, 100%, 50%)").as_deref(), Some("#ff0000ff"));
        assert_eq!(parse_css("hsl(120deg 100% 25%)").as_deref(), Some("#008000ff"));
        assert_eq!(parse_css("hsla(240, 100%, 50%, 0.5)").as_deref(), Some("#0000ff80"));
        assert_eq!(parse_css("hsl(-120, 100%, 50%)").as_deref(), Some("#0000ffff"));
        assert_eq!(parse_css("hsl(60, 0%, 100%)").as_deref(), Some("#ffffffff"));
    }

    #[test]
    fn css_names_and_garbage() {
        assert_eq!(parse_css(" RebeccaPurple ").as_deref(), Some("#663399ff"));
        assert_eq!(parse_css("transparent").as_deref(), Some("#00000000"));
        assert_eq!(parse_css("business"), None, "PlantUML-only names are not CSS");
        for bad in ["", "rgb(1,2)", "rgb(a,b,c)", "rgb(1,2,3,4,5)", "foo(1,2,3)", "rgb(1,2,3", "rgb(nan,0,0)", "hsl(1,2%,3%,x)"] {
            assert_eq!(parse_css(bad), None, "{bad}");
        }
    }
}
