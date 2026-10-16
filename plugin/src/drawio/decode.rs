//! Finds the `<mxGraphModel>` of the requested page in whatever container the file
//! uses: `<mxfile>` with plain or compressed pages, a bare `<mxGraphModel>`, or a
//! `.drawio.svg` that carries the diagram in its `content` attribute.

use base64::engine::{general_purpose::GeneralPurpose, general_purpose::GeneralPurposeConfig, DecodePaddingMode};
use base64::{alphabet, Engine};
use roxmltree::{Document, Node, ParsingOptions};

use crate::Page;

pub fn decode(source: &[u8], page: &Page) -> Result<String, String> {
    let text = std::str::from_utf8(source).map_err(|_| "the draw.io file is not valid UTF-8 text".to_string())?;
    decode_text(text.trim_start_matches('\u{feff}'), page, 0)
}

/// Deepest element nesting accepted. Real diagrams stay in the single digits; the limit
/// only has to keep hostile input from exhausting the stack in code that recurses over
/// the tree.
pub const MAX_XML_DEPTH: usize = 256;

/// Largest inflated size of a compressed page. A few KB of deflate can expand a
/// thousandfold, and the plugin runs inside the Typst process.
const MAX_INFLATED: usize = 64 * 1024 * 1024;

/// Counts element nesting without building a tree, so deeply nested input is refused
/// before the parser (or anything that recurses over its result) sees it. Comments,
/// CDATA, processing instructions and quoted attribute values are skipped so that a `<`
/// or `>` inside them is not miscounted.
pub fn check_xml_depth(text: &str) -> Result<(), String> {
    let b = text.as_bytes();
    let (mut i, mut depth) = (0, 0usize);
    while i < b.len() {
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        let rest = &b[i..];
        if rest.starts_with(b"<!--") {
            i = find_bytes(b, i + 4, b"-->").map_or(b.len(), |p| p + 3);
        } else if rest.starts_with(b"<![CDATA[") {
            i = find_bytes(b, i + 9, b"]]>").map_or(b.len(), |p| p + 3);
        } else if rest.starts_with(b"<?") {
            i = find_bytes(b, i + 2, b"?>").map_or(b.len(), |p| p + 2);
        } else if rest.starts_with(b"<!") {
            // DOCTYPE; an internal subset in brackets can hold `>`.
            let mut j = i + 2;
            let mut brackets = 0;
            while j < b.len() && !(b[j] == b'>' && brackets == 0) {
                match b[j] {
                    b'[' => brackets += 1,
                    b']' if brackets > 0 => brackets -= 1,
                    _ => {}
                }
                j += 1;
            }
            i = j + 1;
        } else {
            let closing = rest.get(1) == Some(&b'/');
            let mut j = i + 1;
            let mut quote = 0u8;
            while j < b.len() {
                match b[j] {
                    q @ (b'"' | b'\'') if quote == 0 => quote = q,
                    q if q == quote => quote = 0,
                    b'>' if quote == 0 => break,
                    _ => {}
                }
                j += 1;
            }
            if closing {
                depth = depth.saturating_sub(1);
            } else if !(j > 0 && b.get(j - 1) == Some(&b'/')) {
                depth += 1;
                if depth > MAX_XML_DEPTH {
                    return Err(format!("the diagram XML is nested too deeply (more than {MAX_XML_DEPTH} levels)"));
                }
            }
            i = j + 1;
        }
    }
    Ok(())
}

fn find_bytes(hay: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    hay.get(from..)?.windows(needle.len()).position(|w| w == needle).map(|p| p + from)
}

fn parse(text: &str) -> Result<Document<'_>, String> {
    if text.trim().is_empty() {
        return Err("the draw.io file is empty".into());
    }
    check_xml_depth(text)?;
    let opts = ParsingOptions { allow_dtd: true, ..Default::default() };
    Document::parse_with_options(text, opts).map_err(|e| format!("the draw.io file is not valid XML: {e}"))
}

fn decode_text(text: &str, page: &Page, depth: u32) -> Result<String, String> {
    let doc = parse(text)?;
    let root = doc.root_element();
    match root.tag_name().name() {
        "mxfile" => {
            let pages: Vec<Node> = root.children().filter(|n| n.has_tag_name("diagram")).collect();
            if pages.is_empty() {
                // A file whose pages were all stripped, or an mxfile that wraps the model directly.
                if let Some(model) = root.children().find(|n| n.has_tag_name("mxGraphModel")) {
                    return select_single(text, model, page);
                }
                return Err("the draw.io file has no <diagram> pages".into());
            }
            let names: Vec<String> = pages
                .iter()
                .enumerate()
                .map(|(i, p)| p.attribute("name").map(str::to_string).unwrap_or_else(|| format!("Page-{}", i + 1)))
                .collect();
            let chosen = match page {
                Page::Index(i) => {
                    if *i >= pages.len() {
                        return Err(format!(
                            "page index {i} is out of range: the file has {} page{} ({})",
                            pages.len(),
                            if pages.len() == 1 { "" } else { "s" },
                            list_pages(&names)
                        ));
                    }
                    *i
                }
                Page::Name(name) => names.iter().position(|n| n == name).ok_or_else(|| {
                    format!("page \"{name}\" not found in the draw.io file; available pages: {}", list_pages(&names))
                })?,
            };
            page_model(text, pages[chosen])
        }
        "mxGraphModel" => select_single(text, root, page),
        "svg" => {
            let content = root.attribute("content").ok_or_else(|| {
                "this SVG does not contain a draw.io diagram (no `content` attribute); export it with \"Include a copy of my diagram\" enabled"
                    .to_string()
            })?;
            if depth > 0 {
                return Err("the draw.io SVG contains another SVG instead of a diagram".into());
            }
            decode_text(content, page, depth + 1)
        }
        other => {
            Err(format!("this is not a draw.io file: expected <mxfile>, <mxGraphModel> or an SVG, found <{other}>"))
        }
    }
}

fn list_pages(names: &[String]) -> String {
    names.iter().enumerate().map(|(i, n)| format!("{i}: \"{n}\"")).collect::<Vec<_>>().join(", ")
}

/// Files without pages hold one diagram, so only the first page can be asked for.
fn select_single(text: &str, model: Node, page: &Page) -> Result<String, String> {
    match page {
        Page::Index(0) => Ok(text[model.range()].to_string()),
        Page::Index(i) => Err(format!("page index {i} is out of range: the file has a single page")),
        Page::Name(name) => Err(format!("page \"{name}\" not found: the file has a single unnamed page")),
    }
}

fn page_model(text: &str, diagram: Node) -> Result<String, String> {
    if let Some(model) = diagram.children().find(|n| n.has_tag_name("mxGraphModel")) {
        return Ok(text[model.range()].to_string());
    }
    let packed: String = diagram.text().unwrap_or("").chars().filter(|c| !c.is_whitespace()).collect();
    if packed.is_empty() {
        return Err("the selected page is empty".into());
    }
    inflate_page(&packed)
}

/// Compressed pages are `base64(deflate-raw(encodeURIComponent(xml)))`.
pub fn inflate_page(packed: &str) -> Result<String, String> {
    let engine = GeneralPurpose::new(
        &alphabet::STANDARD,
        GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
    );
    let deflated = engine.decode(packed).map_err(|e| format!("the compressed page is not valid base64: {e}"))?;
    let raw = miniz_oxide::inflate::decompress_to_vec_with_limit(&deflated, MAX_INFLATED).map_err(|e| {
        if e.status == miniz_oxide::inflate::TINFLStatus::HasMoreOutput {
            format!("the compressed page is too large (it inflates to more than {} MiB)", MAX_INFLATED >> 20)
        } else {
            format!("the compressed page could not be inflated: {:?}", e.status)
        }
    })?;
    let escaped = String::from_utf8(raw).map_err(|_| "the compressed page is not valid UTF-8".to_string())?;
    let xml = percent_encoding::percent_decode_str(&escaped)
        .decode_utf8()
        .map_err(|_| "the compressed page has invalid percent-encoding".to_string())?;
    Ok(xml.into_owned())
}

/// The inverse of `inflate_page`; used by the tests and fixture tooling.
#[cfg(test)]
pub fn deflate_page(xml: &str) -> String {
    use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
    // encodeURIComponent leaves `-_.!~*'()` alone.
    const SET: &percent_encoding::AsciiSet = &NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'_')
        .remove(b'.')
        .remove(b'!')
        .remove(b'~')
        .remove(b'*')
        .remove(b'\'')
        .remove(b'(')
        .remove(b')');
    let escaped = utf8_percent_encode(xml, SET).to_string();
    let deflated = miniz_oxide::deflate::compress_to_vec(escaped.as_bytes(), 6);
    base64::engine::general_purpose::STANDARD.encode(deflated)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODEL: &str = r#"<mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/></root></mxGraphModel>"#;

    fn file(pages: &[(&str, String)]) -> String {
        let mut s = String::from("<mxfile host=\"app.diagrams.net\">");
        for (name, body) in pages {
            s.push_str(&format!("<diagram name=\"{name}\" id=\"x\">{body}</diagram>"));
        }
        s + "</mxfile>"
    }

    #[test]
    fn uncompressed_page_selection() {
        let f = file(&[("First", MODEL.into()), ("Second", MODEL.replace("id=\"1\"", "id=\"2\""))]);
        let a = decode(f.as_bytes(), &Page::Index(0)).unwrap();
        assert!(a.contains("id=\"1\""));
        let b = decode(f.as_bytes(), &Page::Name("Second".into())).unwrap();
        assert!(b.contains("id=\"2\""));
    }

    #[test]
    fn compressed_page_round_trip() {
        let packed = deflate_page(MODEL);
        let f = file(&[("Packed", packed)]);
        assert_eq!(decode(f.as_bytes(), &Page::Index(0)).unwrap(), MODEL);
    }

    #[test]
    fn compressed_page_with_unicode_and_whitespace() {
        let xml = MODEL.replace("<root>", "<root><mxCell id=\"u\" value=\"Gr\u{fc}\u{df}e 100% \u{4e2d}\"/>");
        let packed = deflate_page(&xml);
        let wrapped = format!("\n  {}\n  {}\n", &packed[..10], &packed[10..]);
        let f = file(&[("U", wrapped)]);
        assert_eq!(decode(f.as_bytes(), &Page::Index(0)).unwrap(), xml);
    }

    #[test]
    fn bare_model() {
        assert_eq!(decode(MODEL.as_bytes(), &Page::Index(0)).unwrap(), MODEL);
        assert!(decode(MODEL.as_bytes(), &Page::Index(1)).unwrap_err().contains("out of range"));
        assert!(decode(MODEL.as_bytes(), &Page::Name("x".into())).unwrap_err().contains("not found"));
    }

    #[test]
    fn svg_with_content_attribute() {
        let inner =
            file(&[("P", deflate_page(MODEL))]).replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
        let svg = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \"http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd\">\n<svg xmlns=\"http://www.w3.org/2000/svg\" content=\"{inner}\"><g/></svg>"
        );
        assert_eq!(decode(svg.as_bytes(), &Page::Index(0)).unwrap(), MODEL);
    }

    #[test]
    fn page_errors_list_the_pages() {
        let f = file(&[("Alpha", MODEL.into()), ("Beta", MODEL.into())]);
        let e = decode(f.as_bytes(), &Page::Name("Gamma".into())).unwrap_err();
        assert!(e.contains("\"Gamma\"") && e.contains("\"Alpha\"") && e.contains("\"Beta\""), "{e}");
        let e = decode(f.as_bytes(), &Page::Index(5)).unwrap_err();
        assert!(e.contains("out of range") && e.contains("2 pages") && e.contains("1: \"Beta\""), "{e}");
    }

    #[test]
    fn deflate_bombs_are_refused() {
        // 80 MiB of spaces deflates to a few dozen KB.
        let bomb = miniz_oxide::deflate::compress_to_vec(&vec![b' '; 80 << 20], 9);
        let packed = base64::engine::general_purpose::STANDARD.encode(bomb);
        let f = file(&[("Bomb", packed)]);
        let e = decode(f.as_bytes(), &Page::Index(0)).unwrap_err();
        assert!(e.contains("too large"), "{e}");
    }

    #[test]
    fn deep_nesting_is_refused() {
        let deep = format!("{}{}", "<a>".repeat(600), "</a>".repeat(600));
        assert!(decode(deep.as_bytes(), &Page::Index(0)).unwrap_err().contains("nested too deeply"));
        // Brackets inside attribute values, comments and CDATA do not count.
        let fine = r#"<mxGraphModel a="<<<<" ><!-- <a><a><a> --><root><![CDATA[<x><x>]]></root></mxGraphModel>"#;
        assert!(check_xml_depth(fine).is_ok());
        let wide = format!("<r>{}</r>", "<a/>".repeat(10_000));
        assert!(check_xml_depth(&wide).is_ok());
    }

    #[test]
    fn malformed_input() {
        assert!(decode(b"", &Page::Index(0)).unwrap_err().contains("empty"));
        assert!(decode(b"<mxfile><diagram>", &Page::Index(0)).unwrap_err().contains("not valid XML"));
        assert!(decode(b"<html/>", &Page::Index(0)).unwrap_err().contains("not a draw.io file"));
        assert!(decode(b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>", &Page::Index(0))
            .unwrap_err()
            .contains("content"));
        let bad = file(&[("Bad", "!!!not base64!!!".into())]);
        assert!(decode(bad.as_bytes(), &Page::Index(0)).unwrap_err().contains("base64"));
        let junk = file(&[("Bad", "aGVsbG8gd29ybGQ=".into())]);
        assert!(decode(junk.as_bytes(), &Page::Index(0)).unwrap_err().contains("inflated"));
    }
}
