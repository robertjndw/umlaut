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
    // Colors PlantUML adds on top of the HTML set.
    ("application", 0xffffcc), ("business", 0xffffcc), ("implementation", 0xc0ffc0), ("motivation", 0xd1bddb),
    ("physical", 0xc9ffc9), ("strategy", 0xf8e7c0), ("technology", 0xc9ffc9),
];

pub fn named(name: &str) -> Option<u32> {
    let key: String = name.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase();
    NAMES.iter().find(|(n, _)| *n == key).map(|&(_, v)| v)
}

/// `named` as an opaque `#rrggbbaa` color.
pub fn named_hex(name: &str) -> Option<String> {
    named(name).map(|v| format!("#{v:06x}ff"))
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
}
