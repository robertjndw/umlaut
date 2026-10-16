//! The statements every diagram type understands: title, header, footer, caption,
//! legend, skinparam. They are pulled out of the line list first (in two steps, so the
//! skinparams are complete before any text is turned into runs) and the diagram parser
//! only sees what is left.

use super::creole;
use super::lexer::{block_end, first_word, is_end, rest_after_word, Line};
use super::model::{Ctx, Placement};
use super::skin::Skin;
use crate::scene::HAlign;

#[derive(Debug, Default)]
pub struct Raw {
    title: Option<String>,
    header: Option<(String, HAlign)>,
    footer: Option<(String, HAlign)>,
    caption: Option<String>,
    legend: Option<(String, Placement)>,
}

fn align_word(w: &str) -> Option<HAlign> {
    match w {
        "left" => Some(HAlign::Left),
        "right" => Some(HAlign::Right),
        "center" => Some(HAlign::Center),
        _ => None,
    }
}

fn join(lines: &[&Line]) -> String {
    lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n")
}

/// Removes the page-level statements from `lines` and applies skinparams to `skin`.
/// Returns the remaining lines and the raw texts, to be registered with `register`
/// once the skin is final.
pub fn extract(lines: Vec<Line>, skin: &mut Skin, warnings: &mut Vec<String>) -> Result<(Vec<Line>, Raw), String> {
    let mut out = Vec::new();
    let mut raw = Raw::default();
    let mut cur = super::lexer::Cursor::new(&lines);
    while let Some(l) = cur.next() {
        let lower = l.text.to_ascii_lowercase();
        let w = first_word(&lower);
        let rest = rest_after_word(&l.text);

        if let Some(end) = block_end(&l.text) {
            out.push(l.clone());
            while let Some(n) = cur.next() {
                out.push(n.clone());
                if end(&n.text) {
                    break;
                }
            }
            continue;
        }

        match w {
            "title" => {
                raw.title = Some(if rest.is_empty() { join(&cur.block(l.no, "title", |t| is_end(t, &["title"]))?) } else { rest.to_string() });
            }
            "caption" if !rest.is_empty() => raw.caption = Some(rest.to_string()),
            "header" | "footer" => {
                let (text, align) = text_block(&mut cur, l, w, rest, if w == "header" { HAlign::Right } else { HAlign::Center })?;
                if w == "header" { raw.header = Some((text, align)) } else { raw.footer = Some((text, align)) }
            }
            "left" | "right" | "center" if matches!(first_word(&rest.to_ascii_lowercase()), "header" | "footer") => {
                let which = first_word(&rest.to_ascii_lowercase()).to_string();
                let after = rest_after_word(rest);
                let (text, _) = text_block(&mut cur, l, &which, after, HAlign::Center)?;
                let align = align_word(w).unwrap_or(HAlign::Center);
                if which == "header" { raw.header = Some((text, align)) } else { raw.footer = Some((text, align)) }
            }
            "legend" => {
                let mut place = Placement { h: HAlign::Center, top: false };
                for word in rest.to_ascii_lowercase().split_whitespace() {
                    match word {
                        "top" => place.top = true,
                        "bottom" => place.top = false,
                        other => {
                            if let Some(a) = align_word(other) {
                                place.h = a;
                            }
                        }
                    }
                }
                let body = cur.block(l.no, "legend", |t| is_end(t, &["legend"]))?;
                raw.legend = Some((join(&body), place));
            }
            "skinparam" => parse_skinparam(&mut cur, l, rest, skin, warnings)?,
            "scale" => warnings.push(format!("line {}: scale is ignored, use the scale argument of plantuml()", l.no)),
            _ => out.push(l.clone()),
        }
    }
    Ok((out, raw))
}

/// `header text` on one line, or `header` followed by lines up to `end header`.
fn text_block(cur: &mut super::lexer::Cursor, l: &Line, word: &str, rest: &str, default: HAlign) -> Result<(String, HAlign), String> {
    let text = if rest.is_empty() {
        let w = word.to_string();
        join(&cur.block(l.no, &w, |t| is_end(t, &[word]))?)
    } else {
        rest.to_string()
    };
    Ok((text, default))
}

fn parse_skinparam(cur: &mut super::lexer::Cursor, l: &Line, rest: &str, skin: &mut Skin, warnings: &mut Vec<String>) -> Result<(), String> {
    let mut put = |skin: &mut Skin, key: &str, value: &str| {
        if let Some(w) = skin.set(key, value) {
            warnings.push(format!("line {}: {w}", l.no));
        }
    };
    if let Some(prefix) = rest.strip_suffix('{') {
        let prefix = prefix.trim().to_string();
        while let Some(n) = cur.next() {
            if n.text == "}" {
                return Ok(());
            }
            if n.text.is_empty() {
                continue;
            }
            let (k, v) = split_param(&n.text);
            put(skin, &format!("{prefix}{k}"), v);
        }
        return Err(format!("line {}: skinparam block is never closed", l.no));
    }
    if rest.is_empty() {
        return Err(format!("line {}: skinparam needs a name and a value", l.no));
    }
    let (k, v) = split_param(rest);
    put(skin, k, v);
    Ok(())
}

fn split_param(s: &str) -> (&str, &str) {
    let s = s.trim();
    match s.find(|c: char| c.is_whitespace() || c == '=') {
        Some(i) => (&s[..i], s[i..].trim_start_matches(|c: char| c.is_whitespace() || c == '=')),
        None => (s, ""),
    }
}

/// Registers the page-level texts, in a fixed order: title, header, footer, caption,
/// legend. Call once the skin is complete.
pub fn register(raw: Raw, ctx: &mut Ctx) {
    let sk = ctx.skin.clone();
    let runs = |text: &str, base| creole::runs(text, &base);
    if let Some(t) = raw.title {
        let r = runs(&t, sk.text(&["title"], 14.0, "#000000").bold());
        ctx.meta.title = ctx.reg.add_opt(r, None);
    }
    if let Some((t, a)) = raw.header {
        let r = runs(&t, sk.text(&["header"], 10.0, "#000000"));
        ctx.meta.header = ctx.reg.add_opt(r, None).map(|id| (id, a));
    }
    if let Some((t, a)) = raw.footer {
        let r = runs(&t, sk.text(&["footer"], 10.0, "#000000"));
        ctx.meta.footer = ctx.reg.add_opt(r, None).map(|id| (id, a));
    }
    if let Some(t) = raw.caption {
        let r = runs(&t, sk.text(&["caption"], 14.0, "#000000"));
        ctx.meta.caption = ctx.reg.add_opt(r, None);
    }
    if let Some((t, p)) = raw.legend {
        let r = runs(&t, sk.text(&["legend"], 15.0, "#000000"));
        ctx.meta.legend = ctx.reg.add_opt(r, None).map(|id| (id, p));
    }
}
