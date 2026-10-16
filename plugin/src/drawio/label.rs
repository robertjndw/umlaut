//! Cell labels: plain text and the HTML subset draw.io writes, converted to runs, plus a
//! rough text measure so labels outside their shape and edge labels get a sensible box.

use crate::scene::{find_seq, hex, Run};

use super::style::{parse_color, Style};

/// Character formatting while walking the label.
#[derive(Debug, Clone)]
pub struct TextProps {
    pub size: f32,
    pub color: String,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub font: Option<String>,
    pub link: Option<String>,
    /// `<pre>` or `white-space: pre`: spaces and tabs stay as written.
    pub pre: bool,
}

impl TextProps {
    pub fn from_style(style: &Style) -> TextProps {
        let bits = style.num("fontStyle", 0.0) as u32;
        let mut color = style.color("fontColor", Some("#000000")).unwrap_or_else(|| "#000000ff".into());
        let opacity = style.num("textOpacity", 100.0).clamp(0.0, 100.0);
        if opacity < 100.0 {
            color = scale_alpha(&color, opacity / 100.0);
        }
        TextProps {
            size: style.num("fontSize", 12.0).clamp(1.0, MAX_FONT_SIZE),
            color,
            bold: bits & 1 != 0,
            italic: bits & 2 != 0,
            underline: bits & 4 != 0,
            strike: bits & 8 != 0,
            font: first_family(style.str_or("fontFamily", "Helvetica")),
            link: None,
            pre: false,
        }
    }

    fn run(&self, text: String) -> Run {
        Run {
            text,
            bold: self.bold,
            italic: self.italic,
            underline: self.underline,
            strike: self.strike,
            size: self.size,
            color: self.color.clone(),
            font: self.font.clone(),
            link: self.link.clone(),
            br: false,
        }
    }
}

/// Typst lays out whatever size it is given; an absurd one only costs time and memory.
const MAX_FONT_SIZE: f32 = 1000.0;

/// Multiplies the alpha of a `#rrggbbaa` color.
pub fn scale_alpha(color: &str, factor: f32) -> String {
    if color.len() != 9 || factor >= 1.0 {
        return color.to_string();
    }
    let a = u8::from_str_radix(&color[7..9], 16).unwrap_or(255) as f32;
    format!("{}{:02x}", &color[..7], (a * factor.max(0.0)).round() as u8)
}

/// The first family of a CSS-style list. `None` when it is empty, because Typst warns
/// about an unknown font family called "".
fn first_family(list: &str) -> Option<String> {
    let first = list.split(',').next().unwrap_or(list).trim();
    let family = first.trim_matches(|c| c == '"' || c == '\'').trim();
    (!family.is_empty()).then(|| family.to_string())
}

/// Plain labels: newlines are line breaks, everything else is literal.
pub fn plain_runs(value: &str, base: &TextProps) -> Vec<Run> {
    let mut runs = Vec::new();
    for (i, line) in value.replace("\r\n", "\n").split('\n').enumerate() {
        if i > 0 {
            runs.push(Run::br());
        }
        if !line.is_empty() {
            runs.push(base.run(line.to_string()));
        }
    }
    trim_runs(runs)
}

/// Removes leading and trailing line breaks so an empty first or last line does not
/// push the text off-center.
fn trim_runs(mut runs: Vec<Run>) -> Vec<Run> {
    while runs.last().is_some_and(|r| r.br) {
        runs.pop();
    }
    while runs.first().is_some_and(|r| r.br) {
        runs.remove(0);
    }
    runs
}

pub fn is_blank(runs: &[Run]) -> bool {
    runs.iter().all(|r| r.br || r.text.trim().is_empty())
}

// HTML ------------------------------------------------------------------------------

const BLOCKS: &[&str] = &[
    "div",
    "p",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "li",
    "ul",
    "ol",
    "table",
    "tr",
    "pre",
    "blockquote",
    "center",
    "hr",
];

struct Builder {
    lines: Vec<Vec<Run>>,
    cur: Vec<Run>,
    /// The line so far ends in a collapsible space.
    at_space: bool,
}

impl Builder {
    fn line_has_content(&self) -> bool {
        self.cur.iter().any(|r| !r.text.is_empty())
    }

    fn text(&mut self, raw: &str, props: &TextProps) {
        if props.pre {
            // Typst collapses runs of ordinary spaces, so preserved ones become
            // non-breaking spaces (which also matches `pre` not wrapping).
            let kept: String = raw
                .chars()
                .flat_map(|c| match c {
                    ' ' => vec!['\u{a0}'],
                    '\t' => vec!['\u{a0}'; 4],
                    _ => vec![c],
                })
                .collect();
            if !kept.is_empty() {
                self.cur.push(props.run(kept));
                self.at_space = false;
            }
            return;
        }
        let mut out = String::new();
        for ch in raw.chars() {
            if matches!(ch, ' ' | '\n' | '\r' | '\t') {
                if !self.at_space && self.line_has_content() || (!self.at_space && !out.is_empty()) {
                    out.push(' ');
                    self.at_space = true;
                }
            } else {
                out.push(ch);
                self.at_space = false;
            }
        }
        if !out.is_empty() {
            self.cur.push(props.run(out));
        }
    }

    fn end_line(&mut self) {
        if let Some(last) = self.cur.last_mut() {
            let trimmed = last.text.trim_end_matches(' ').len();
            last.text.truncate(trimmed);
        }
        self.cur.retain(|r| !r.text.is_empty());
        self.lines.push(std::mem::take(&mut self.cur));
        self.at_space = false;
    }

    /// Table cells on one row sit next to each other, so they need a gap to stay apart.
    fn cell_break(&mut self, props: &TextProps) {
        if self.line_has_content() && !self.at_space {
            self.cur.push(props.run("\u{2003}".to_string()));
            self.at_space = true;
        }
    }

    /// A block element starts a new line only when the current one has text; this is
    /// what makes `<div>a</div><div>b</div>` two lines rather than three.
    fn block_break(&mut self) {
        if self.line_has_content() {
            self.end_line();
        }
    }
}

pub fn html_runs(value: &str, base: &TextProps) -> Vec<Run> {
    // draw.io turns linefeeds in HTML labels into `<br>` before rendering.
    let value = value.replace("\r\n", "<br>").replace('\n', "<br>");
    let value = value.as_str();
    let mut b = Builder { lines: Vec::new(), cur: Vec::new(), at_space: false };
    let mut stack: Vec<(String, TextProps)> = Vec::new();
    let mut props = base.clone();
    let mut ordered: Vec<u32> = Vec::new();
    let chars: Vec<char> = value.chars().collect();
    let mut i = 0;
    let mut text = String::new();

    macro_rules! flush {
        () => {
            if !text.is_empty() {
                b.text(&decode_entities(&text), &props);
                text.clear();
            }
        };
    }

    while i < chars.len() {
        if chars[i] != '<' {
            text.push(chars[i]);
            i += 1;
            continue;
        }
        // A `<` that does not start a tag is literal text (draw.io escapes real ones).
        let next = chars.get(i + 1).copied().unwrap_or(' ');
        if !(next.is_ascii_alphabetic() || next == '/' || next == '!') {
            text.push('<');
            i += 1;
            continue;
        }
        if chars[i..].starts_with(&['<', '!', '-', '-']) {
            let end = find_seq(&chars, i + 4, &['-', '-', '>']).map(|p| p + 3).unwrap_or(chars.len());
            i = end;
            continue;
        }
        let (tag, end) = read_tag(&chars, i);
        i = end;
        flush!();
        let name = tag.name.as_str();
        if tag.closing {
            if let Some(pos) = stack.iter().rposition(|(n, _)| n == name) {
                let (_, saved) = stack[pos].clone();
                stack.truncate(pos);
                props = saved;
            }
            if BLOCKS.contains(&name) {
                b.block_break();
            }
            if name == "ol" {
                ordered.pop();
            }
            continue;
        }
        match name {
            "br" => {
                b.end_line();
                continue;
            }
            "img" | "wbr" | "input" | "hr" => {
                if name == "hr" {
                    b.block_break();
                }
                continue;
            }
            _ => {}
        }
        if matches!(name, "script" | "style") {
            // Their content is code, not label text.
            if !tag.self_closing {
                i = skip_raw_element(&chars, i, name);
            }
            continue;
        }
        if BLOCKS.contains(&name) {
            b.block_break();
        }
        if matches!(name, "td" | "th") {
            b.cell_break(&props);
        }
        let saved = props.clone();
        match name {
            "b" | "strong" => props.bold = true,
            "i" | "em" | "cite" | "var" => props.italic = true,
            "u" | "ins" => props.underline = true,
            "s" | "strike" | "del" => props.strike = true,
            "a" => {
                if let Some(href) = tag.attr("href") {
                    props.link = Some(decode_entities(href));
                }
                props.underline = true;
                if tag.attr("style").is_none_or(|s| !s.contains("color")) {
                    props.color = hex("#0000ee");
                }
            }
            "font" => {
                if let Some(c) = tag.attr("color") {
                    if let Some(c) = parse_color(c, None) {
                        props.color = c;
                    }
                }
                if let Some(s) = tag.attr("size") {
                    props.size = font_size_attr(s, props.size);
                }
                if let Some(f) = tag.attr("face").and_then(first_family) {
                    props.font = Some(f);
                }
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                props.bold = true;
                props.size = base.size * [2.0, 1.5, 1.17, 1.0, 0.83, 0.67][(name.as_bytes()[1] - b'1') as usize];
            }
            "ol" => ordered.push(0),
            "li" => match ordered.last_mut() {
                Some(n) => {
                    *n += 1;
                    let marker = format!("{n}. ");
                    b.cur.push(props.run(marker));
                }
                None => b.cur.push(props.run("\u{2022} ".to_string())),
            },
            "pre" => props.pre = true,
            "sup" | "sub" => props.size = (props.size * 0.8).max(1.0),
            _ => {}
        }
        if let Some(style) = tag.attr("style") {
            apply_css(style, &mut props);
        }
        if !tag.self_closing {
            stack.push((name.to_string(), saved));
        }
    }
    flush!();
    b.end_line();

    let mut lines = b.lines;
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    let mut runs = Vec::new();
    for (n, line) in lines.into_iter().enumerate() {
        if n > 0 {
            runs.push(Run::br());
        }
        runs.extend(line);
    }
    trim_runs(runs)
}

/// Index just past the closing tag of a `<script>` or `<style>` element whose opening
/// tag ended before `from`; the end of input when it is never closed.
fn skip_raw_element(chars: &[char], from: usize, name: &str) -> usize {
    let close: Vec<char> = format!("</{name}").chars().collect();
    let mut p = from;
    while p + close.len() <= chars.len() {
        if chars[p..p + close.len()].iter().zip(&close).all(|(a, b)| a.to_ascii_lowercase() == *b) {
            return chars[p..].iter().position(|&c| c == '>').map_or(chars.len(), |e| p + e + 1);
        }
        p += 1;
    }
    chars.len()
}

struct Tag {
    name: String,
    closing: bool,
    self_closing: bool,
    attrs: Vec<(String, String)>,
}

impl Tag {
    fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
}

fn read_tag(chars: &[char], start: usize) -> (Tag, usize) {
    let mut i = start + 1;
    let closing = chars.get(i) == Some(&'/');
    if closing {
        i += 1;
    }
    let mut name = String::new();
    while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '>' && chars[i] != '/' {
        name.push(chars[i].to_ascii_lowercase());
        i += 1;
    }
    let mut attrs = Vec::new();
    let mut self_closing = false;
    while i < chars.len() && chars[i] != '>' {
        if chars[i].is_whitespace() {
            i += 1;
        } else if chars[i] == '/' {
            self_closing = true;
            i += 1;
        } else {
            let mut key = String::new();
            while i < chars.len() && !matches!(chars[i], '=' | '>' | '/') && !chars[i].is_whitespace() {
                key.push(chars[i].to_ascii_lowercase());
                i += 1;
            }
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            let mut value = String::new();
            if chars.get(i) == Some(&'=') {
                i += 1;
                while i < chars.len() && chars[i].is_whitespace() {
                    i += 1;
                }
                match chars.get(i) {
                    Some(&q) if q == '"' || q == '\'' => {
                        i += 1;
                        while i < chars.len() && chars[i] != q {
                            value.push(chars[i]);
                            i += 1;
                        }
                        i += 1;
                    }
                    _ => {
                        while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '>' {
                            value.push(chars[i]);
                            i += 1;
                        }
                    }
                }
            }
            if !key.is_empty() {
                attrs.push((key, value));
            }
        }
    }
    (Tag { name, closing, self_closing, attrs }, (i + 1).min(chars.len()))
}

fn font_size_attr(value: &str, current: f32) -> f32 {
    let v = value.trim();
    if let Some(rel) = v.strip_prefix('+').and_then(|n| n.parse::<i32>().ok()) {
        return html_size(3 + rel).unwrap_or(current);
    }
    if let Some(rel) = v.strip_prefix('-').and_then(|n| n.parse::<i32>().ok()) {
        return html_size(3 - rel).unwrap_or(current);
    }
    v.parse::<i32>().ok().and_then(html_size).unwrap_or(current)
}

/// The sizes browsers give `<font size=1..7>`.
fn html_size(n: i32) -> Option<f32> {
    Some(match n.clamp(1, 7) {
        1 => 10.0,
        2 => 13.0,
        3 => 16.0,
        4 => 18.0,
        5 => 24.0,
        6 => 32.0,
        _ => 48.0,
    })
}

fn apply_css(css: &str, props: &mut TextProps) {
    for decl in css.split(';') {
        let Some((k, v)) = decl.split_once(':') else { continue };
        let v = v.trim();
        match k.trim().to_ascii_lowercase().as_str() {
            "color" => {
                if let Some(c) = parse_css_color(v) {
                    props.color = c;
                }
            }
            "font-size" => {
                if let Some(px) = css_length(v, props.size) {
                    props.size = px.clamp(1.0, MAX_FONT_SIZE);
                }
            }
            "font-weight" => props.bold = v == "bold" || v == "bolder" || v.parse::<u32>().is_ok_and(|w| w >= 600),
            "font-style" => props.italic = v.starts_with("italic") || v.starts_with("oblique"),
            "text-decoration" | "text-decoration-line" => {
                props.underline = v.contains("underline");
                props.strike = v.contains("line-through");
            }
            "font-family" => {
                if let Some(f) = first_family(v) {
                    props.font = Some(f);
                }
            }
            "white-space" => props.pre = matches!(v, "pre" | "pre-wrap" | "break-spaces"),
            _ => {}
        }
    }
}

fn parse_css_color(v: &str) -> Option<String> {
    if let Some(inner) = v.strip_prefix("rgb(").or_else(|| v.strip_prefix("rgba(")).and_then(|r| r.strip_suffix(')')) {
        let parts: Vec<f32> = inner
            .split([',', ' ', '/'])
            .filter(|p| !p.is_empty())
            .filter_map(|p| p.trim_end_matches('%').parse::<f32>().ok())
            .collect();
        if parts.len() >= 3 {
            let a = parts.get(3).copied().unwrap_or(1.0).clamp(0.0, 1.0);
            return Some(format!(
                "#{:02x}{:02x}{:02x}{:02x}",
                parts[0] as u8,
                parts[1] as u8,
                parts[2] as u8,
                (a * 255.0).round() as u8
            ));
        }
        return None;
    }
    parse_color(v, None)
}

fn css_length(v: &str, current: f32) -> Option<f32> {
    css_length_raw(v, current).filter(|n| n.is_finite())
}

fn css_length_raw(v: &str, current: f32) -> Option<f32> {
    let v = v.trim();
    if let Some(n) = v.strip_suffix("px") {
        return n.trim().parse().ok();
    }
    if let Some(n) = v.strip_suffix("pt") {
        return n.trim().parse::<f32>().ok().map(|p| p * 4.0 / 3.0);
    }
    if let Some(n) = v.strip_suffix("em") {
        return n.trim().parse::<f32>().ok().map(|e| e * current);
    }
    if let Some(n) = v.strip_suffix('%') {
        return n.trim().parse::<f32>().ok().map(|p| p / 100.0 * current);
    }
    match v {
        "small" => Some(13.0),
        "medium" => Some(16.0),
        "large" => Some(18.0),
        "x-large" => Some(24.0),
        "xx-large" => Some(32.0),
        "x-small" => Some(10.0),
        "larger" => Some(current * 1.2),
        "smaller" => Some(current / 1.2),
        _ => v.parse().ok(),
    }
}

pub fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find('&') {
        out.push_str(&rest[..pos]);
        rest = &rest[pos..];
        let end = rest.find(';').filter(|&e| e <= 10);
        let decoded = end.and_then(|e| entity(&rest[1..e]).map(|c| (c, e)));
        match decoded {
            Some((c, e)) => {
                out.push_str(&c);
                rest = &rest[e + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Latin-1 names in code point order from U+00A0.
const LATIN1: [&str; 96] = [
    "nbsp", "iexcl", "cent", "pound", "curren", "yen", "brvbar", "sect", "uml", "copy", "ordf", "laquo", "not", "shy",
    "reg", "macr", "deg", "plusmn", "sup2", "sup3", "acute", "micro", "para", "middot", "cedil", "sup1", "ordm",
    "raquo", "frac14", "frac12", "frac34", "iquest", "Agrave", "Aacute", "Acirc", "Atilde", "Auml", "Aring", "AElig",
    "Ccedil", "Egrave", "Eacute", "Ecirc", "Euml", "Igrave", "Iacute", "Icirc", "Iuml", "ETH", "Ntilde", "Ograve",
    "Oacute", "Ocirc", "Otilde", "Ouml", "times", "Oslash", "Ugrave", "Uacute", "Ucirc", "Uuml", "Yacute", "THORN",
    "szlig", "agrave", "aacute", "acirc", "atilde", "auml", "aring", "aelig", "ccedil", "egrave", "eacute", "ecirc",
    "euml", "igrave", "iacute", "icirc", "iuml", "eth", "ntilde", "ograve", "oacute", "ocirc", "otilde", "ouml",
    "divide", "oslash", "ugrave", "uacute", "ucirc", "uuml", "yacute", "thorn", "yuml",
];

/// Greek capitals from U+0391 (U+03A2 is unassigned) and small letters from U+03B1.
const GREEK_UPPER: [&str; 25] = [
    "Alpha", "Beta", "Gamma", "Delta", "Epsilon", "Zeta", "Eta", "Theta", "Iota", "Kappa", "Lambda", "Mu", "Nu", "Xi",
    "Omicron", "Pi", "Rho", "", "Sigma", "Tau", "Upsilon", "Phi", "Chi", "Psi", "Omega",
];
const GREEK_LOWER: [&str; 25] = [
    "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa", "lambda", "mu", "nu", "xi",
    "omicron", "pi", "rho", "sigmaf", "sigma", "tau", "upsilon", "phi", "chi", "psi", "omega",
];

/// The rest of HTML 4's entities that show up in real labels: punctuation, arrows and
/// math. Written as escapes so the source stays ASCII.
const OTHER: &[(&str, u32)] = &[
    ("amp", 0x26), ("lt", 0x3c), ("gt", 0x3e), ("quot", 0x22), ("apos", 0x27),
    ("OElig", 0x152), ("oelig", 0x153), ("Scaron", 0x160), ("scaron", 0x161), ("Yuml", 0x178), ("fnof", 0x192),
    ("circ", 0x2c6), ("tilde", 0x2dc),
    ("ensp", 0x2002), ("emsp", 0x2003), ("thinsp", 0x2009), ("zwnj", 0x200c), ("zwj", 0x200d), ("lrm", 0x200e),
    ("rlm", 0x200f), ("ndash", 0x2013), ("mdash", 0x2014), ("lsquo", 0x2018), ("rsquo", 0x2019), ("sbquo", 0x201a),
    ("ldquo", 0x201c), ("rdquo", 0x201d), ("bdquo", 0x201e), ("dagger", 0x2020), ("Dagger", 0x2021),
    ("bull", 0x2022), ("hellip", 0x2026), ("permil", 0x2030), ("prime", 0x2032), ("Prime", 0x2033),
    ("lsaquo", 0x2039), ("rsaquo", 0x203a), ("oline", 0x203e), ("frasl", 0x2044), ("euro", 0x20ac),
    ("trade", 0x2122), ("larr", 0x2190), ("uarr", 0x2191), ("rarr", 0x2192), ("darr", 0x2193), ("harr", 0x2194),
    ("crarr", 0x21b5), ("lArr", 0x21d0), ("uArr", 0x21d1), ("rArr", 0x21d2), ("dArr", 0x21d3), ("hArr", 0x21d4),
    ("forall", 0x2200), ("part", 0x2202), ("exist", 0x2203), ("empty", 0x2205), ("nabla", 0x2207),
    ("isin", 0x2208), ("notin", 0x2209), ("ni", 0x220b), ("prod", 0x220f), ("sum", 0x2211), ("minus", 0x2212),
    ("lowast", 0x2217), ("radic", 0x221a), ("prop", 0x221d), ("infin", 0x221e), ("ang", 0x2220), ("and", 0x2227),
    ("or", 0x2228), ("cap", 0x2229), ("cup", 0x222a), ("int", 0x222b), ("there4", 0x2234), ("sim", 0x223c),
    ("cong", 0x2245), ("asymp", 0x2248), ("ne", 0x2260), ("equiv", 0x2261), ("le", 0x2264), ("ge", 0x2265),
    ("sub", 0x2282), ("sup", 0x2283), ("nsub", 0x2284), ("sube", 0x2286), ("supe", 0x2287), ("oplus", 0x2295),
    ("otimes", 0x2297), ("perp", 0x22a5), ("sdot", 0x22c5), ("lceil", 0x2308), ("rceil", 0x2309),
    ("lfloor", 0x230a), ("rfloor", 0x230b), ("loz", 0x25ca), ("spades", 0x2660), ("clubs", 0x2663),
    ("hearts", 0x2665), ("diams", 0x2666),
];

fn entity(name: &str) -> Option<String> {
    if let Some(num) = name.strip_prefix('#') {
        let code = match num.strip_prefix(['x', 'X']) {
            Some(h) => u32::from_str_radix(h, 16).ok()?,
            None => num.parse().ok()?,
        };
        // NUL is never valid text and the C1 range is not what browsers show either.
        return char::from_u32(code).filter(|c| *c != '\0').map(|c| c.to_string());
    }
    if name.is_empty() {
        return None;
    }
    let code = if let Some(i) = LATIN1.iter().position(|n| *n == name) {
        0xa0 + i as u32
    } else if let Some(i) = GREEK_UPPER.iter().position(|n| *n == name) {
        0x391 + i as u32
    } else if let Some(i) = GREEK_LOWER.iter().position(|n| *n == name) {
        0x3b1 + i as u32
    } else {
        OTHER.iter().find(|(n, _)| *n == name)?.1
    };
    char::from_u32(code).map(|c| c.to_string())
}

// Measuring -------------------------------------------------------------------------

/// Helvetica advance widths (per 1000 em) for ASCII 32..=126.
const HELVETICA: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, // space .. /
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, // 0 .. ?
    1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, // @ .. O
    667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556, // P .. _
    333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, // ` .. o
    556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584, // p .. ~
];

pub fn text_width(text: &str, size: f32, bold: bool) -> f32 {
    let em: f32 = text
        .chars()
        .map(|c| match c {
            ' '..='~' => HELVETICA[c as usize - 32] as f32,
            '\u{a0}' => 278.0,
            // CJK and other wide scripts are roughly square, everything else Latin-ish.
            c if (c as u32) >= 0x2e80 => 1000.0,
            _ => 556.0,
        })
        .sum();
    em / 1000.0 * size * if bold { 1.06 } else { 1.0 }
}

/// Size of the runs laid out in lines. With `wrap` the lines break at spaces to stay
/// within the width (a single long word still overflows).
pub fn measure(runs: &[Run], wrap: Option<f32>) -> (f32, f32) {
    let mut width = 0.0f32;
    let mut height = 0.0f32;
    let mut line_w = 0.0f32;
    let mut line_h = 0.0f32;
    let mut first_on_line = true;
    let last_size = runs.iter().rev().find(|r| !r.br).map(|r| r.size).unwrap_or(12.0);

    let newline = |line_w: &mut f32, line_h: &mut f32, size: f32, width: &mut f32, height: &mut f32| {
        *width = width.max(*line_w);
        *height += if *line_h > 0.0 { *line_h } else { size * 1.2 };
        *line_w = 0.0;
        *line_h = 0.0;
    };

    for run in runs {
        if run.br {
            newline(&mut line_w, &mut line_h, last_size, &mut width, &mut height);
            first_on_line = true;
            continue;
        }
        line_h = line_h.max(run.size * 1.2);
        // Words keep their trailing space so the break lands between words.
        for word in run.text.split_inclusive(' ') {
            let w = text_width(word, run.size, run.bold);
            let trimmed = text_width(word.trim_end_matches(' '), run.size, run.bold);
            if let Some(max) = wrap {
                if !first_on_line && line_w + trimmed > max {
                    newline(&mut line_w, &mut line_h, run.size, &mut width, &mut height);
                    line_h = run.size * 1.2;
                }
            }
            line_w += w;
            first_on_line = false;
        }
    }
    if line_w > 0.0 || line_h > 0.0 || height == 0.0 {
        newline(&mut line_w, &mut line_h, last_size, &mut width, &mut height);
    }
    (width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> TextProps {
        TextProps::from_style(&Style::parse(""))
    }

    fn texts(runs: &[Run]) -> Vec<String> {
        runs.iter().map(|r| if r.br { "|".to_string() } else { r.text.clone() }).collect()
    }

    #[test]
    fn nested_formatting() {
        let runs = html_runs("plain <b>bold <i>both</i></b> <u>under</u>", &base());
        assert_eq!(texts(&runs), ["plain ", "bold ", "both", " ", "under"]);
        assert!(!runs[0].bold);
        assert!(runs[1].bold && !runs[1].italic);
        assert!(runs[2].bold && runs[2].italic);
        assert!(runs[4].underline);
    }

    #[test]
    fn br_and_blocks_make_lines() {
        assert_eq!(texts(&html_runs("a<br>b", &base())), ["a", "|", "b"]);
        assert_eq!(texts(&html_runs("<div>a</div><div>b</div>", &base())), ["a", "|", "b"]);
        assert_eq!(texts(&html_runs("a<div>b</div>", &base())), ["a", "|", "b"]);
        // A `<br>` in an otherwise empty block is a blank line.
        assert_eq!(texts(&html_runs("<div>a</div><div><br></div><div>b</div>", &base())), ["a", "|", "|", "b"]);
        // A trailing `<br>` does not add a line.
        assert_eq!(texts(&html_runs("<div>a<br></div>", &base())), ["a"]);
    }

    #[test]
    fn font_and_css() {
        let runs = html_runs(
            "<font color=\"#ff0000\" size=\"5\" face=\"Courier New\">x</font><span style=\"color: rgb(0, 128, 0); font-size: 20px; font-weight: bold; font-style: italic; text-decoration: line-through;\">y</span>",
            &base(),
        );
        assert_eq!(runs[0].color, "#ff0000ff");
        assert_eq!(runs[0].size, 24.0);
        assert_eq!(runs[0].font.as_deref(), Some("Courier New"));
        assert_eq!(runs[1].color, "#008000ff");
        assert_eq!(runs[1].size, 20.0);
        assert!(runs[1].bold && runs[1].italic && runs[1].strike);
    }

    #[test]
    fn entities_and_whitespace() {
        let runs = html_runs("a&nbsp;&nbsp;b &amp;   c  d &lt;e&gt; &#65;&#x42;", &base());
        assert_eq!(runs.iter().map(|r| r.text.as_str()).collect::<String>(), "a\u{a0}\u{a0}b & c d <e> AB");
        assert_eq!(texts(&html_runs("x\ny", &base())), ["x", "|", "y"]);
    }

    #[test]
    fn unclosed_and_stray_markup() {
        let runs = html_runs("1 < 2 <b>bold", &base());
        assert_eq!(runs.iter().map(|r| r.text.as_str()).collect::<String>(), "1 < 2 bold");
        assert!(runs.last().unwrap().bold);
    }

    #[test]
    fn plain_text_lines() {
        let runs = plain_runs("one\ntwo\n", &base());
        assert_eq!(texts(&runs), ["one", "|", "two"]);
    }

    #[test]
    fn font_style_bits_and_opacity() {
        let p = TextProps::from_style(&Style::parse("fontStyle=5;fontColor=#ff0000;textOpacity=50;fontFamily=Verdana"));
        assert!(p.bold && !p.italic && p.underline && !p.strike);
        assert_eq!(p.color, "#ff000080");
        assert_eq!(p.font.as_deref(), Some("Verdana"));
    }

    #[test]
    fn measure_wraps_on_spaces() {
        let runs = plain_runs("aaaa bbbb cccc dddd", &base());
        let (w1, h1) = measure(&runs, None);
        let (w2, h2) = measure(&runs, Some(w1 / 2.0));
        assert!(w2 < w1);
        assert!(h2 > h1 * 1.5);
    }

    #[test]
    fn list_items() {
        let runs = html_runs("<ul><li>a</li><li>b</li></ul>", &base());
        assert_eq!(texts(&runs), ["\u{2022} ", "a", "|", "\u{2022} ", "b"]);
    }

    fn joined(runs: &[Run]) -> String {
        runs.iter().map(|r| if r.br { "|".to_string() } else { r.text.clone() }).collect()
    }

    #[test]
    fn script_and_style_contents_are_dropped() {
        let runs = html_runs("a<style>.x { color: red }</style>b<SCRIPT type=\"x\">if (1 < 2) alert('<b>')</script>c", &base());
        assert_eq!(joined(&runs), "abc");
        assert_eq!(joined(&html_runs("x<script>never closed", &base())), "x");
    }

    #[test]
    fn pre_keeps_whitespace() {
        let runs = html_runs("<pre>a  b\n  c</pre> after", &base());
        assert_eq!(joined(&runs), "a\u{a0}\u{a0}b|\u{a0}\u{a0}c|after");
        let styled = html_runs("<div style=\"white-space: pre\">x   y</div>", &base());
        assert_eq!(joined(&styled), "x\u{a0}\u{a0}\u{a0}y");
    }

    #[test]
    fn table_cells_and_rows_are_separated() {
        let runs = html_runs("<table><tr><td>a</td><td>b</td></tr><tr><td>c</td><td>d</td></tr></table>", &base());
        assert_eq!(joined(&runs), "a\u{2003}b|c\u{2003}d");
    }

    #[test]
    fn named_and_numeric_entities() {
        assert_eq!(
            decode_entities("&eacute;&uuml;&auml;&ouml;&szlig;&euro;&copy;&reg;&trade;&hellip;&mdash;&ndash;"),
            "\u{e9}\u{fc}\u{e4}\u{f6}\u{df}\u{20ac}\u{a9}\u{ae}\u{2122}\u{2026}\u{2014}\u{2013}"
        );
        assert_eq!(
            decode_entities("&laquo;&raquo;&ldquo;&rdquo;&lsquo;&rsquo;&bull;&middot;&times;&divide;&deg;&plusmn;&frac12;&para;&sect;"),
            "\u{ab}\u{bb}\u{201c}\u{201d}\u{2018}\u{2019}\u{2022}\u{b7}\u{d7}\u{f7}\u{b0}\u{b1}\u{bd}\u{b6}\u{a7}"
        );
        assert_eq!(decode_entities("&yuml;&Alpha;&omega;&#x1F600;&#128512;"), "\u{ff}\u{391}\u{3c9}\u{1f600}\u{1f600}");
        assert_eq!(decode_entities("&bogus; &#0; &#xD800; &; & x"), "&bogus; &#0; &#xD800; &; & x");
    }

    #[test]
    fn hostile_css_values_stay_finite() {
        let runs = html_runs("<span style=\"font-size:1e999px\">a</span><span style=\"font-size:-5px\">b</span>", &base());
        assert!(runs.iter().all(|r| r.size.is_finite() && r.size >= 1.0));
        let runs = html_runs("<span style=\"font-size:9999999px\">a</span>", &base());
        assert_eq!(runs[0].size, MAX_FONT_SIZE);
    }

    #[test]
    fn empty_font_family_is_none() {
        assert_eq!(first_family(""), None);
        assert_eq!(first_family(" , Arial"), None);
        assert_eq!(first_family("'Fira Code', mono").as_deref(), Some("Fira Code"));
        let p = TextProps::from_style(&Style::parse("fontFamily=,Arial"));
        assert_eq!(p.font, None);
        let runs = html_runs("<font face=\"\">x</font>", &base());
        assert_eq!(runs[0].font.as_deref(), Some("Helvetica"));
    }
}
