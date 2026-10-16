//! PlantUML's text markup (Creole plus a few HTML-like tags) turned into scene runs.
//!
//! Supported: `**bold**`, `//italic//`, `__underline__`, `--strike--`, `""mono""`,
//! `<b> <i> <u> <s> <del> <strike>`, `<color:x>`, `<size:N>`, `<font color=x size=N>`,
//! `<U+XXXX>`, `\n` and real newlines as line breaks, `\t`, and `~` to escape the next
//! character. A marker without a partner stays literal text, which keeps things like
//! `a--b` and URLs readable. Everything else (images, sprites, tables, lists) stays
//! plain text.

use super::colors::parse_color;
use crate::scene::{find_seq, Run};
pub use crate::scene::plain_text as plain;

/// Family used for `""mono""` text; the renderer falls back to the document font if the
/// machine has none of these.
pub const MONO: &str = "DejaVu Sans Mono";

/// Starting style of a piece of text before any markup applies.
#[derive(Debug, Clone, PartialEq)]
pub struct Base {
    pub size: f32,
    pub color: String,
    pub bold: bool,
    pub italic: bool,
    pub font: Option<String>,
}

impl Base {
    pub fn new(size: f32, color: &str) -> Base {
        Base { size, color: crate::scene::hex(color), bold: false, italic: false, font: None }
    }

    pub fn bold(mut self) -> Base {
        self.bold = true;
        self
    }

    pub fn italic(mut self) -> Base {
        self.italic = true;
        self
    }
}

#[derive(Clone)]
struct State {
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    mono: bool,
    colors: Vec<String>,
    sizes: Vec<f32>,
    fonts: Vec<String>,
    font_frames: Vec<(bool, bool, bool)>,
}

struct Builder<'a> {
    base: &'a Base,
    st: State,
    runs: Vec<Run>,
    buf: String,
}

impl Builder<'_> {
    fn flush(&mut self) {
        if self.buf.is_empty() {
            return;
        }
        let st = &self.st;
        let mut run = Run::new(std::mem::take(&mut self.buf))
            .size(*st.sizes.last().unwrap_or(&self.base.size))
            .color(st.colors.last().unwrap_or(&self.base.color));
        // `bold` and `italic` are flips relative to the base style, so `**x**` inside an
        // already bold title reads as plain.
        run.bold = st.bold ^ self.base.bold;
        run.italic = st.italic ^ self.base.italic;
        run.underline = st.underline;
        run.strike = st.strike;
        run.font = if st.mono {
            Some(MONO.into())
        } else {
            st.fonts.last().cloned().or_else(|| self.base.font.clone())
        };
        self.runs.push(run);
    }

    fn br(&mut self) {
        self.flush();
        self.runs.push(Run::br());
    }
}

fn tag_at(chars: &[char], i: usize) -> Option<(String, usize)> {
    // `<<stereotype>>` is text, not a tag.
    if chars.get(i + 1) == Some(&'<') {
        return None;
    }
    let end = (i + 1..chars.len().min(i + 80)).find(|&j| chars[j] == '>')?;
    let body: String = chars[i + 1..end].iter().collect();
    if body.contains('<') || body.is_empty() {
        return None;
    }
    Some((body, end + 1))
}

fn attr(body: &str, name: &str) -> Option<String> {
    let lower = body.to_ascii_lowercase();
    let pos = lower.find(&format!("{name}="))? + name.len() + 1;
    let rest = &body[pos..];
    let value: String = if let Some(q) = rest.strip_prefix('"') {
        q.chars().take_while(|&c| c != '"').collect()
    } else {
        rest.chars().take_while(|c| !c.is_whitespace()).collect()
    };
    Some(value)
}

/// Marks that toggle a style when they have a partner later in the text (or are
/// currently open).
fn toggle(b: &mut Builder, chars: &[char], i: usize, marker: [char; 2], get: fn(&mut State) -> &mut bool) -> bool {
    let open = *get(&mut b.st);
    if open || find_seq(chars, i + 2, &marker).is_some() {
        b.flush();
        let v = get(&mut b.st);
        *v = !*v;
        true
    } else {
        false
    }
}

/// Runs for `text`. Newlines (real or written `\n`) become `Run::br()`.
pub fn runs(text: &str, base: &Base) -> Vec<Run> {
    let chars: Vec<char> = text.chars().collect();
    let mut b = Builder {
        base,
        st: State {
            bold: false,
            italic: false,
            underline: false,
            strike: false,
            mono: false,
            colors: vec![],
            sizes: vec![],
            fonts: vec![],
            font_frames: vec![],
        },
        runs: Vec::new(),
        buf: String::new(),
    };
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        match (c, next) {
            ('~', Some(n)) => {
                b.buf.push(n);
                i += 2;
            }
            ('\n', _) => {
                b.br();
                i += 1;
            }
            ('\\', Some('n')) | ('\\', Some('l')) | ('\\', Some('r')) => {
                b.br();
                i += 2;
            }
            ('\\', Some('t')) => {
                b.buf.push_str("    ");
                i += 2;
            }
            ('*', Some('*')) if toggle(&mut b, &chars, i, ['*', '*'], |s| &mut s.bold) => i += 2,
            ('/', Some('/')) if i > 0 && chars[i - 1] != ':' && toggle(&mut b, &chars, i, ['/', '/'], |s| &mut s.italic) => i += 2,
            ('/', Some('/')) if i == 0 && toggle(&mut b, &chars, i, ['/', '/'], |s| &mut s.italic) => i += 2,
            ('_', Some('_')) if toggle(&mut b, &chars, i, ['_', '_'], |s| &mut s.underline) => i += 2,
            ('-', Some('-')) if toggle(&mut b, &chars, i, ['-', '-'], |s| &mut s.strike) => i += 2,
            ('"', Some('"')) if toggle(&mut b, &chars, i, ['"', '"'], |s| &mut s.mono) => i += 2,
            ('<', _) => match tag_at(&chars, i) {
                Some((body, end)) if apply_tag(&mut b, &body) => i = end,
                _ => {
                    b.buf.push('<');
                    i += 1;
                }
            },
            _ => {
                b.buf.push(c);
                i += 1;
            }
        }
    }
    b.flush();
    b.runs
}

/// Applies a tag; false when it is not one of ours, so the caller keeps it as text.
fn apply_tag(b: &mut Builder, body: &str) -> bool {
    let lower = body.trim().to_ascii_lowercase();
    if let Some(hexcode) = lower.strip_prefix("u+") {
        if let Some(ch) = u32::from_str_radix(hexcode, 16).ok().and_then(char::from_u32) {
            b.buf.push(ch);
            return true;
        }
        return false;
    }
    let (name, arg) = match lower.split_once([':', ' ']) {
        Some((n, _)) => (n.to_string(), body.trim()[n.len() + 1..].trim().to_string()),
        None => (lower.clone(), String::new()),
    };
    let closing = name.starts_with('/');
    let name = name.trim_start_matches('/');
    match name {
        "b" | "i" | "u" | "s" | "del" | "strike" => {
            b.flush();
            let on = !closing;
            match name {
                "b" => b.st.bold = on && !b.base.bold,
                "i" => b.st.italic = on && !b.base.italic,
                "u" => b.st.underline = on,
                _ => b.st.strike = on,
            }
            true
        }
        "color" | "colour" => {
            b.flush();
            if closing {
                b.st.colors.pop();
                true
            } else if let Some(c) = parse_color(&arg) {
                b.st.colors.push(c);
                true
            } else {
                false
            }
        }
        "size" => {
            b.flush();
            if closing {
                b.st.sizes.pop();
                true
            } else if let Ok(n) = arg.parse::<f32>() {
                b.st.sizes.push(n);
                true
            } else {
                false
            }
        }
        "font" => {
            b.flush();
            if closing {
                if let Some((c, z, f)) = b.st.font_frames.pop() {
                    if c {
                        b.st.colors.pop();
                    }
                    if z {
                        b.st.sizes.pop();
                    }
                    if f {
                        b.st.fonts.pop();
                    }
                }
                return true;
            }
            let c = attr(body, "color").and_then(|c| parse_color(&c));
            let z = attr(body, "size").and_then(|s| s.parse::<f32>().ok());
            let f = attr(body, "face");
            b.st.font_frames.push((c.is_some(), z.is_some(), f.is_some()));
            b.st.colors.extend(c);
            b.st.sizes.extend(z);
            b.st.fonts.extend(f);
            true
        }
        "back" | "w" | "plain" | "code" => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Base {
        Base::new(12.0, "#000000")
    }

    #[test]
    fn bold_italic_underline_strike() {
        let r = runs("a **b** //c// __d__ --e--", &base());
        let styled: Vec<_> = r.iter().filter(|r| r.bold || r.italic || r.underline || r.strike).map(|r| r.text.as_str()).collect();
        assert_eq!(styled, ["b", "c", "d", "e"]);
        assert_eq!(plain(&r), "a b c d e");
    }

    #[test]
    fn unpaired_markers_stay_text() {
        assert_eq!(plain(&runs("a--b and **c", &base())), "a--b and **c");
        assert_eq!(plain(&runs("see http://x.y/z", &base())), "see http://x.y/z");
    }

    #[test]
    fn breaks_and_escapes() {
        let r = runs("one\\ntwo\nthree ~**x~**", &base());
        assert_eq!(plain(&r), "one\ntwo\nthree **x**");
        assert_eq!(r.iter().filter(|r| r.br).count(), 2);
    }

    #[test]
    fn color_size_tags() {
        let r = runs("<color:red>r</color> <size:20>big</size> <font color=#00f>f</font>", &base());
        assert_eq!(r[0].color, "#ff0000ff");
        assert_eq!(r.iter().find(|r| r.text == "big").unwrap().size, 20.0);
        assert_eq!(r.iter().find(|r| r.text == "f").unwrap().color, "#0000ffff");
        assert_eq!(r.iter().find(|r| r.text == " ").unwrap().color, "#000000ff");
    }

    #[test]
    fn html_tags_and_mono() {
        let r = runs("<b>x</b><i>y</i>\"\"code\"\" <U+00E9>", &base());
        assert!(r[0].bold && !r[1].bold && r[1].italic);
        assert_eq!(r.iter().find(|r| r.text == "code").unwrap().font.as_deref(), Some(MONO));
        assert_eq!(plain(&r), "xycode é");
    }

    #[test]
    fn stereotype_brackets_are_text() {
        assert_eq!(plain(&runs("<<Foo>> a < b", &base())), "<<Foo>> a < b");
    }

    #[test]
    fn base_style_survives_tags() {
        let b = Base::new(12.0, "#000000").bold();
        let r = runs("a <i>b</i> c", &b);
        assert!(r.iter().all(|r| r.bold));
        assert!(r[1].italic && !r[2].italic);
    }
}
