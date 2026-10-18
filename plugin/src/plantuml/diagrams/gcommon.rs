//! What the graph diagrams (class, state, use case and component) parse the same way:
//! entity references, arrows with their decorations, links with cardinalities and
//! labels, notes, `#color` styles and `<<stereotypes>>`.
//!
//! The functions here only split text; registering labels and creating entities is up
//! to each diagram, because the defaults differ (`A --> B` makes two classes in a class
//! diagram, two actors in a use case diagram).

use crate::plantuml::colors::{parse_color, parse_paint};
use crate::plantuml::layout::graph::Dir;
use crate::plantuml::lexer::{err, first_word, is_end, rest_after_word, Cursor, Line};
use crate::plantuml::model::LabelId;
use crate::scene::Paint;

/// What decorates one end of a link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Deco {
    None,
    /// `>` and `<`.
    Arrow,
    /// `|>`: hollow triangle, inheritance.
    Triangle,
    /// `o`: hollow diamond, aggregation.
    Diamond,
    /// `*`: filled diamond, composition.
    DiamondFilled,
    /// `+`: circled plus, nested class.
    Plus,
    /// `x`: not navigable.
    Cross,
    /// `#`: square.
    Hash,
    /// `}` and `{`: crow's foot.
    Crowfoot,
    /// `()`: provided interface.
    Lollipop,
    /// `)`: required interface.
    Socket,
    /// A ring at the end; Mermaid's `--o`.
    Circle,
    /// A plain filled triangle; Mermaid's `-->`.
    Point,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Arrow {
    /// At the first entity.
    pub tail: Deco,
    /// At the second entity.
    pub head: Deco,
    pub dashed: bool,
    pub bold: bool,
    /// Number of body characters: `->` is 1, `-->` is 2.
    pub len: usize,
    pub dir: Option<Dir>,
    pub color: Option<String>,
    pub width: Option<f32>,
    pub hidden: bool,
}

impl Arrow {
    pub fn plain(len: usize) -> Arrow {
        Arrow { tail: Deco::None, head: Deco::None, dashed: false, bold: false, len, dir: None, color: None, width: None, hidden: false }
    }
}

/// A link between two entities by index into the diagram's entity list.
#[derive(Debug, Clone)]
pub struct Link {
    pub from: usize,
    pub to: usize,
    pub arrow: Arrow,
    pub label: Option<LabelId>,
    /// A trailing `>` or `<` on the label: a small arrow beside the text, forward or back.
    pub label_dir: Option<bool>,
    pub from_card: Option<LabelId>,
    pub to_card: Option<LabelId>,
    pub line: usize,
}

/// An entity of a graph diagram. `D` is the diagram's own `Def` enum.
#[derive(Debug, Clone)]
pub struct Ent<D> {
    pub key: String,
    pub parent: Option<usize>,
    pub def: D,
}

/// A reference to an entity in a statement, before it is resolved.
#[derive(Debug, Clone, PartialEq)]
pub enum Ref {
    /// `Name` or `"Quoted name"`.
    Name(String),
    /// `[Component]`; also `[*]` as `Star`.
    Bracket(String),
    /// `(Use case)`.
    Paren(String),
    /// `:Actor:`.
    Colon(String),
    /// `[*]`.
    Star,
    /// `() Name` or a bare `()`.
    Interface(Option<String>),
}

fn ident_len(s: &str) -> usize {
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    let mut i = 0;
    while i < chars.len() {
        let (_, c) = chars[i];
        let next = chars.get(i + 1).map(|x| x.1);
        let ok = c.is_alphanumeric()
            || c == '_'
            || c == '$'
            // A dot or `::` joins namespace segments but `..` is an arrow.
            || (c == '.' && next.is_some_and(|n| n.is_alphanumeric() || n == '_'))
            || (c == ':' && next == Some(':') && chars.get(i + 2).is_some_and(|x| x.1.is_alphanumeric()))
            || (c == ':' && i > 0 && chars[i - 1].1 == ':');
        if !ok {
            break;
        }
        i += 1;
    }
    chars.get(i).map_or(s.len(), |x| x.0)
}

/// Reads one entity reference from the start of `s`.
pub fn scan_ref(s: &str) -> Option<(Ref, &str)> {
    let s = s.trim_start();
    let first = s.chars().next()?;
    match first {
        '"' => {
            let end = s[1..].find('"')? + 1;
            Some((Ref::Name(s[1..end].to_string()), &s[end + 1..]))
        }
        '[' => {
            if let Some(rest) = s.strip_prefix("[*]") {
                return Some((Ref::Star, rest));
            }
            let end = s.find(']')?;
            Some((Ref::Bracket(s[1..end].trim().to_string()), &s[end + 1..]))
        }
        '(' => {
            if let Some(after) = s.strip_prefix("()") {
                let rest = after.trim_start();
                // `() Name` declares an interface in place; `()-` is a decoration.
                if rest.starts_with('-') || rest.starts_with('.') || rest.is_empty() {
                    return None;
                }
                let (name, rest) = match rest.strip_prefix('"') {
                    Some(q) => {
                        let end = q.find('"')?;
                        (q[..end].to_string(), &q[end + 1..])
                    }
                    None => {
                        let n = ident_len(rest);
                        if n == 0 {
                            return None;
                        }
                        (rest[..n].to_string(), &rest[n..])
                    }
                };
                return Some((Ref::Interface(Some(name)), rest));
            }
            let mut depth = 0;
            for (i, c) in s.char_indices() {
                match c {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some((Ref::Paren(s[1..i].trim().to_string()), &s[i + 1..]));
                        }
                    }
                    _ => {}
                }
            }
            None
        }
        ':' => {
            let end = s[1..].find(':')? + 1;
            Some((Ref::Colon(s[1..end].trim().to_string()), &s[end + 1..]))
        }
        _ => {
            let n = ident_len(s);
            if n == 0 {
                return None;
            }
            // `.Name` is the global namespace's `Name`.
            Some((Ref::Name(s[..n].trim_start_matches('.').to_string()), &s[n..]))
        }
    }
}

/// The arrow token at the start of `s` and what follows it.
fn scan_arrow(s: &str) -> Option<(&str, &str)> {
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    let mut i = 0;
    let mut prev = ' ';
    while i < chars.len() {
        let (_, c) = chars[i];
        let next = chars.get(i + 1).map(|x| x.1);
        let ok = match c {
            '-' | '.' | '=' | '~' | '<' | '>' | '|' | '*' | '+' | '#' | '}' | '{' | '^' | '(' | ')' | '0' => true,
            '[' => {
                let close = chars[i..].iter().position(|x| x.1 == ']')?;
                i += close;
                true
            }
            'o' | 'x' if i == 0 => matches!(next, Some('-') | Some('.')),
            c if c.is_ascii_lowercase() => matches!(prev, '-' | '.') || prev.is_ascii_lowercase(),
            _ => false,
        };
        if !ok {
            break;
        }
        prev = chars[i].1;
        i += 1;
    }
    let end = chars.get(i).map_or(s.len(), |x| x.0);
    let tok = &s[..end];
    if tok.is_empty() || !tok.contains(['-', '.', '=', '~']) {
        return None;
    }
    Some((tok, &s[end..]))
}

fn dir_word(w: &str) -> Option<Dir> {
    match w {
        "up" | "u" => Some(Dir::Up),
        "down" | "do" | "d" => Some(Dir::Down),
        "left" | "le" | "l" => Some(Dir::Left),
        "right" | "ri" | "r" => Some(Dir::Right),
        _ => None,
    }
}

/// Parses an arrow token such as `<|--`, `*-->`, `-up->`, `-[#red,dashed]->`.
pub fn parse_arrow(tok: &str) -> Option<Arrow> {
    let mut arrow = Arrow::plain(0);
    let mut body = tok.to_string();
    if let Some(a) = body.find('[') {
        let b = body[a..].find(']')? + a;
        let style = body[a + 1..b].to_string();
        body.replace_range(a..=b, "");
        for part in style.split(',') {
            let part = part.trim();
            let lower = part.to_ascii_lowercase();
            match lower.as_str() {
                "dashed" | "dotted" => arrow.dashed = true,
                "bold" => {
                    arrow.bold = true;
                    arrow.width = Some(2.0);
                }
                "plain" => arrow.dashed = false,
                "hidden" => arrow.hidden = true,
                _ => {
                    if let Some(w) = lower.strip_prefix("thickness=") {
                        arrow.width = w.parse().ok();
                    } else if let Some(c) = parse_color(part) {
                        arrow.color = Some(c);
                    }
                }
            }
        }
    }
    // A direction word sits between body characters: `-up->`, `.right.>`.
    let chars: Vec<char> = body.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_lowercase() {
            let mut j = i;
            while j < chars.len() && chars[j].is_ascii_lowercase() {
                j += 1;
            }
            let word: String = chars[i..j].iter().collect();
            let flanked = i > 0 && matches!(chars[i - 1], '-' | '.') && j < chars.len() && matches!(chars[j], '-' | '.');
            if flanked {
                if let Some(d) = dir_word(&word) {
                    arrow.dir = Some(d);
                    let mut v = chars.clone();
                    v.drain(i..j);
                    body = v.into_iter().collect();
                    break;
                }
            }
            i = j;
        } else {
            i += 1;
        }
    }
    // Interface lollipops and sockets: `()-`, `-()`, `-0)-`, `-(0-`, `-(0)-`.
    for (pat, tail, head) in [("(0)", Deco::Lollipop, Deco::Socket), ("0)", Deco::Lollipop, Deco::Socket), ("(0", Deco::Socket, Deco::Lollipop)] {
        if body.contains(pat) {
            body = body.replacen(pat, "", 1);
            arrow.tail = tail;
            arrow.head = head;
            break;
        }
    }
    if arrow.tail == Deco::None {
        if let Some(r) = body.strip_prefix("()") {
            arrow.tail = Deco::Lollipop;
            body = r.to_string();
        } else if let Some(r) = body.strip_suffix("()") {
            arrow.head = Deco::Lollipop;
            body = r.to_string();
        }
    }
    // `-(` and `)-` are bare sockets.
    if arrow.tail == Deco::None {
        if let Some(r) = body.strip_suffix('(') {
            arrow.head = Deco::Socket;
            body = r.to_string();
        } else if let Some(r) = body.strip_prefix(')') {
            arrow.tail = Deco::Socket;
            body = r.to_string();
        }
    }
    if arrow.tail == Deco::None && arrow.head == Deco::None {
        let is_body = |c: char| matches!(c, '-' | '.' | '=' | '~');
        let b: Vec<char> = body.chars().collect();
        let mut lo = 0;
        let mut hi = b.len();
        let starts = |from: usize, p: &str| b[from..].iter().take(p.len()).collect::<String>() == p;
        for (p, d) in [("<|", Deco::Triangle), ("<", Deco::Arrow), ("*", Deco::DiamondFilled), ("o", Deco::Diamond), ("+", Deco::Plus), ("#", Deco::Hash), ("x", Deco::Cross), ("}", Deco::Crowfoot), ("^", Deco::Triangle)] {
            if starts(lo, p) && b.get(lo + p.len()).is_some_and(|&c| is_body(c) || (p == "<" && c == '|')) {
                arrow.tail = d;
                lo += p.len();
                break;
            }
        }
        for (p, d) in [("|>", Deco::Triangle), (">", Deco::Arrow), ("*", Deco::DiamondFilled), ("o", Deco::Diamond), ("+", Deco::Plus), ("#", Deco::Hash), ("x", Deco::Cross), ("{", Deco::Crowfoot), ("^", Deco::Triangle)] {
            let n = p.len();
            if hi > lo + n && b[hi - n..hi].iter().collect::<String>() == p && is_body(b[hi - n - 1]) {
                arrow.head = d;
                hi -= n;
                break;
            }
        }
        body = b[lo..hi].iter().collect();
    }
    if body.is_empty() || !body.chars().all(|c| matches!(c, '-' | '.' | '=' | '~')) {
        return None;
    }
    arrow.len = body.chars().count();
    arrow.dashed |= body.contains(['.', '~']);
    if body.contains('=') {
        arrow.bold = true;
        arrow.width.get_or_insert(2.0);
    }
    Some(arrow)
}

#[derive(Debug, Clone, PartialEq)]
pub struct LinkParts {
    pub left: Ref,
    pub left_card: Option<String>,
    pub arrow: Arrow,
    pub right_card: Option<String>,
    pub right: Ref,
    pub label: Option<String>,
    pub label_dir: Option<bool>,
}

/// Splits `A "1" *-- "many" B : label >`. `None` when the line is not a link.
pub fn parse_link(s: &str) -> Option<LinkParts> {
    let s = s.trim();
    let (left, rest) = if s.starts_with("()") && s[2..].trim_start().starts_with(['-', '.']) {
        return None;
    } else {
        scan_ref(s)?
    };
    let mut rest = rest.trim_start();
    let mut left_card = None;
    if let Some(q) = rest.strip_prefix('"') {
        let end = q.find('"')?;
        left_card = Some(q[..end].to_string());
        rest = q[end + 1..].trim_start();
    }
    let (tok, rest) = scan_arrow(rest)?;
    let arrow = parse_arrow(tok)?;
    let mut rest = rest.trim_start();
    let mut right_card = None;
    if let Some(q) = rest.strip_prefix('"') {
        let end = q.find('"')?;
        let after = q[end + 1..].trim_start();
        // `A --> "B"` is a quoted name; `A --> "1" B` carries a cardinality.
        if !after.is_empty() && !after.starts_with(':') {
            right_card = Some(q[..end].to_string());
            rest = after;
        }
    }
    let (right, rest) = scan_ref(rest)?;
    let rest = rest.trim();
    let (label, label_dir) = if rest.is_empty() {
        (None, None)
    } else {
        let text = rest.strip_prefix(':')?.trim();
        let mut dir = None;
        let mut text = text.to_string();
        if let Some(t) = text.strip_prefix("< ").or_else(|| text.strip_prefix("> ")) {
            dir = Some(text.starts_with('>'));
            text = t.trim_start().to_string();
        } else if let Some(t) = text.strip_suffix('>') {
            if t.is_empty() || t.ends_with(' ') {
                dir = Some(true);
                text = t.trim_end().to_string();
            }
        } else if let Some(t) = text.strip_suffix('<') {
            if t.is_empty() || t.ends_with(' ') {
                dir = Some(false);
                text = t.trim_end().to_string();
            }
        }
        (if text.is_empty() { None } else { Some(text) }, dir)
    };
    Some(LinkParts { left, left_card, arrow, right_card, right, label, label_dir })
}

// ---- styles --------------------------------------------------------------------------------

/// The look an entity or group asks for with `#fill;line:color;line.dashed;text:color`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Look {
    pub fill: Option<Paint>,
    pub line: Option<String>,
    pub text: Option<String>,
    pub dashed: bool,
    pub bold: bool,
}

/// Parses one `#...` token.
pub fn parse_look(tok: &str) -> Option<Look> {
    let t = tok.strip_prefix('#')?;
    let mut look = Look::default();
    let mut parts = t.split(';');
    let head = parts.next().unwrap_or("");
    if !head.is_empty() {
        look.fill = Some(parse_paint(head)?);
    }
    for p in parts {
        let p = p.trim();
        let lower = p.to_ascii_lowercase();
        if let Some(c) = lower.strip_prefix("line:") {
            look.line = parse_color(&p[p.len() - c.len()..]);
        } else if let Some(c) = lower.strip_prefix("text:") {
            look.text = parse_color(&p[p.len() - c.len()..]);
        } else if lower == "line.dashed" || lower == "line.dotted" {
            look.dashed = true;
        } else if lower == "line.bold" {
            look.bold = true;
        }
    }
    Some(look)
}

/// Removes a trailing `#color` token (outside quotes) and parses it.
pub fn take_look(s: &str) -> (&str, Look) {
    let t = s.trim_end();
    if let Some(pos) = t.rfind(" #").or(if t.starts_with('#') { Some(0) } else { None }) {
        let tok = t[pos..].trim();
        if !tok.contains('"') {
            if let Some(look) = parse_look(tok) {
                return (t[..pos].trim_end(), look);
            }
        }
    }
    (t, Look::default())
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stereo {
    /// `<< (C,#FF7700) Name >>`: letter and color of the circle.
    pub spot: Option<(char, Option<String>)>,
    pub text: String,
}

/// Removes every `<<...>>` from `s`. Returns the rest and the stereotypes in order.
pub fn take_stereos(s: &str) -> (String, Vec<Stereo>) {
    let mut rest = String::new();
    let mut found = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let tail = &s[i..];
        if let Some(r) = tail.strip_prefix("<<") {
            if let Some(end) = r.find(">>") {
                let inner = r[..end].trim();
                let mut st = Stereo { spot: None, text: inner.to_string() };
                if let Some(after) = inner.strip_prefix('(') {
                    if let Some(close) = after.find(')') {
                        let mut it = after[..close].splitn(2, ',');
                        if let Some(letter) = it.next().and_then(|l| l.trim().chars().next()) {
                            st.spot = Some((letter, it.next().and_then(|c| parse_color(c.trim()))));
                            st.text = after[close + 1..].trim().to_string();
                        }
                    }
                }
                found.push(st);
                i += 2 + end + 2;
                continue;
            }
        }
        let c = tail.chars().next().unwrap_or(' ');
        rest.push(c);
        i += c.len_utf8();
    }
    (rest.split_whitespace().collect::<Vec<_>>().join(" "), found)
}

// ---- groups --------------------------------------------------------------------------------

/// The outline a container is drawn with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupStyle {
    /// Tab with the name on top of a rectangle, `package` and `namespace`.
    Package,
    Folder,
    Frame,
    /// A 3D box.
    Node,
    Cloud,
    Database,
    /// Name centered at the top, plain rectangle.
    Rectangle,
}

impl GroupStyle {
    pub fn from_word(w: &str) -> Option<GroupStyle> {
        Some(match w.to_ascii_lowercase().as_str() {
            "package" | "namespace" => GroupStyle::Package,
            "folder" => GroupStyle::Folder,
            "frame" => GroupStyle::Frame,
            "node" => GroupStyle::Node,
            "cloud" => GroupStyle::Cloud,
            "database" => GroupStyle::Database,
            "rectangle" | "card" | "artifact" | "storage" | "component" | "agent" | "stack" | "queue" | "file" | "hexagon" | "person" | "boundary" | "control" | "entity" | "usecase" | "interface" | "label" => GroupStyle::Rectangle,
            _ => return None,
        })
    }
}

// ---- notes ---------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum NoteWhere {
    /// `note left of X`; `None` is the previous entity (`note left : text`).
    Of(Option<Ref>),
    /// `note on link`, the previous link.
    OnLink,
    /// `note "text" as N` or `note as N`.
    Floating,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NoteSyntax {
    pub place: NoteWhere,
    pub side: Dir,
    pub text: String,
    pub look: Look,
    pub alias: Option<String>,
}

/// Parses a `note ...` statement, reading follow-up lines from `cur` for the block
/// form. `Ok(None)` when the line is not a note.
pub fn parse_note(l: &Line, cur: &mut Cursor) -> Result<Option<NoteSyntax>, String> {
    let lower = l.text.to_ascii_lowercase();
    let mut rest = l.text.as_str();
    let mut word = first_word(&lower);
    if word == "floating" {
        rest = rest_after_word(rest);
        word = first_word(&lower[lower.len() - rest.len()..]);
    }
    if word != "note" {
        return Ok(None);
    }
    let mut rest = rest_after_word(rest).trim();
    let mut side = None;
    // `note left: text` has no space before the colon.
    let w: String = rest.chars().take_while(|c| c.is_ascii_alphabetic()).collect::<String>().to_ascii_lowercase();
    if matches!(w.as_str(), "left" | "right" | "top" | "bottom") {
        side = Some(match w.as_str() {
            "left" => Dir::Left,
            "right" => Dir::Right,
            "top" => Dir::Up,
            _ => Dir::Down,
        });
        rest = rest[w.len()..].trim();
    }
    let mut place = NoteWhere::Of(None);
    let mut alias = None;
    let mut text_inline: Option<String> = None;
    let lower_rest = rest.to_ascii_lowercase();
    if lower_rest.starts_with("of ") || lower_rest == "of" {
        let after = rest[2..].trim_start();
        let Some((r, tail)) = scan_ref(after) else { return Err(format!("line {}: cannot understand which entity the note belongs to in \"{}\"", l.no, l.text)) };
        place = NoteWhere::Of(Some(r));
        rest = tail.trim();
    } else if lower_rest.starts_with("on link") {
        place = NoteWhere::OnLink;
        rest = rest[7..].trim();
    } else if let Some(q) = rest.strip_prefix('"') {
        let end = q.find('"').ok_or_else(|| format!("line {}: the note's text is missing its closing quote", l.no))?;
        text_inline = Some(q[..end].to_string());
        let tail = q[end + 1..].trim();
        let Some(a) = tail.strip_prefix("as ").or_else(|| tail.strip_prefix("AS ")) else { return Err(format!("line {}: expected `as` and a name after the note's text", l.no)) };
        alias = Some(a.trim().to_string());
        place = NoteWhere::Floating;
        rest = "";
    } else if lower_rest.starts_with("as ") {
        alias = Some(rest[3..].trim().to_string());
        place = NoteWhere::Floating;
        rest = "";
    }
    let (rest, look) = {
        // `#color` may sit before the colon.
        match rest.find(':') {
            Some(c) => {
                let (head, look) = take_look(&rest[..c]);
                if !head.is_empty() {
                    return Err(format!("line {}: cannot understand \"{}\" in the note", l.no, head));
                }
                (&rest[c..], look)
            }
            None => {
                let (head, look) = take_look(rest);
                if !head.is_empty() {
                    return Err(format!("line {}: cannot understand \"{}\" in the note", l.no, head));
                }
                (head, look)
            }
        }
    };
    let text = if let Some(t) = text_inline {
        t
    } else if let Some(t) = rest.strip_prefix(':') {
        t.trim().to_string()
    } else {
        let lines = cur.block(l.no, "this note", |t| is_end(t, &["note"]))?;
        lines.iter().map(|x| x.text.as_str()).collect::<Vec<_>>().join("\n")
    };
    Ok(Some(NoteSyntax { place, side: side.unwrap_or(Dir::Right), text, look, alias }))
}

#[derive(Debug, Clone)]
pub struct Note {
    pub label: LabelId,
    pub look: Look,
    /// The link it hangs on, for `note on link`.
    pub on_link: Option<usize>,
}

/// Where a note goes: `(entity it points at, link it hangs on)`. `last` is the previous
/// entity and `links` the number of links so far. `resolve` looks up the `X` of
/// `note of X`, since each diagram creates missing entities its own way.
pub fn note_target(
    place: &NoteWhere,
    last: Option<usize>,
    links: usize,
    no: usize,
    resolve: impl FnOnce(&Ref) -> Result<usize, String>,
) -> Result<(Option<usize>, Option<usize>), String> {
    Ok(match place {
        NoteWhere::Of(Some(r)) => (Some(resolve(r)?), None),
        NoteWhere::Of(None) => match last {
            Some(i) => (Some(i), None),
            None => return err(no, "this note has nothing to attach to"),
        },
        NoteWhere::OnLink => {
            if links == 0 {
                return err(no, "`note on link` needs a link before it");
            }
            (None, Some(links - 1))
        }
        NoteWhere::Floating => (None, None),
    })
}

/// The dashed connector from a note to its target. The note sits on `side` of the
/// target, so the link runs the other way.
pub fn note_link(note: usize, target: usize, side: Dir, line: usize) -> Link {
    let mut arrow = Arrow::plain(2);
    arrow.dashed = true;
    arrow.dir = Some(side.opposite());
    Link { from: note, to: target, arrow, label: None, label_dir: None, from_card: None, to_card: None, line }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plantuml::lexer;

    fn arrow(t: &str) -> Arrow {
        parse_arrow(t).unwrap_or_else(|| panic!("{t}"))
    }

    #[test]
    fn arrow_decorations() {
        let a = arrow("<|--");
        assert_eq!((a.tail, a.head, a.len, a.dashed), (Deco::Triangle, Deco::None, 2, false));
        let a = arrow("..|>");
        assert_eq!((a.tail, a.head, a.dashed), (Deco::None, Deco::Triangle, true));
        assert_eq!(arrow("*--").tail, Deco::DiamondFilled);
        assert_eq!(arrow("--o").head, Deco::Diamond);
        assert_eq!(arrow("o--o").tail, Deco::Diamond);
        assert_eq!(arrow("-->").head, Deco::Arrow);
        let a = arrow("->");
        assert_eq!((a.len, a.head), (1, Deco::Arrow));
        assert_eq!(arrow("<->").tail, Deco::Arrow);
        assert_eq!(arrow("#--").tail, Deco::Hash);
        assert_eq!(arrow("x--").tail, Deco::Cross);
        assert_eq!(arrow("}--").tail, Deco::Crowfoot);
        assert_eq!(arrow("+--").tail, Deco::Plus);
        assert_eq!(arrow("--").len, 2);
        assert_eq!(arrow("---").len, 3);
        assert!(parse_arrow("abc").is_none());
        assert!(parse_arrow("->>x").is_none());
    }

    #[test]
    fn arrow_styles_and_directions() {
        let a = arrow("-up->");
        assert_eq!((a.dir, a.head), (Some(Dir::Up), Deco::Arrow));
        assert_eq!(arrow(".right.>").dir, Some(Dir::Right));
        assert_eq!(arrow("-le-|>").dir, Some(Dir::Left));
        let a = arrow("-[#red,dashed]->");
        assert_eq!(a.color.as_deref(), Some("#ff0000ff"));
        assert!(a.dashed);
        assert!(arrow("-[hidden]->").hidden);
        let a = arrow("-[#blue,thickness=3]down->");
        assert_eq!((a.width, a.dir), (Some(3.0), Some(Dir::Down)));
        assert_eq!(arrow("-0)-").head, Deco::Socket);
        assert_eq!(arrow("()-").tail, Deco::Lollipop);
        assert_eq!(arrow("-(").head, Deco::Socket);
    }

    #[test]
    fn links() {
        let p = parse_link("Foo \"1\" *-- \"many\" Bar : has >").unwrap();
        assert_eq!(p.left, Ref::Name("Foo".into()));
        assert_eq!((p.left_card.as_deref(), p.right_card.as_deref()), (Some("1"), Some("many")));
        assert_eq!(p.arrow.tail, Deco::DiamondFilled);
        assert_eq!((p.label.as_deref(), p.label_dir), (Some("has"), Some(true)));
        let p = parse_link("A-->B").unwrap();
        assert_eq!((p.left, p.right), (Ref::Name("A".into()), Ref::Name("B".into())));
        let p = parse_link("[*] --> State1").unwrap();
        assert_eq!(p.left, Ref::Star);
        let p = parse_link(":User: --> (Login) : uses").unwrap();
        assert_eq!((p.left, p.right), (Ref::Colon("User".into()), Ref::Paren("Login".into())));
        let p = parse_link("a.b.C ..> \"Quoted Name\"").unwrap();
        assert_eq!((p.left, p.right), (Ref::Name("a.b.C".into()), Ref::Name("Quoted Name".into())));
        assert!(p.right_card.is_none());
        let p = parse_link("[Web] -down-> [DB]").unwrap();
        assert_eq!(p.arrow.dir, Some(Dir::Down));
        assert!(parse_link("A : text -> x").is_none());
        assert!(parse_link("class A").is_none());
        assert!(parse_link("A --> B junk").is_none());
        let p = parse_link("Foo o-- Bar").unwrap();
        assert_eq!(p.arrow.tail, Deco::Diamond);
    }

    #[test]
    fn looks_and_stereotypes() {
        let (rest, look) = take_look("class A #red");
        assert_eq!(rest, "class A");
        assert_eq!(look.fill.unwrap().color, "#ff0000ff");
        let (_, look) = take_look("A #lightblue;line:red;line.dashed;text:blue");
        assert_eq!((look.line.as_deref(), look.text.as_deref(), look.dashed), (Some("#ff0000ff"), Some("#0000ffff"), true));
        assert_eq!(take_look("class A").1, Look::default());
        let (rest, st) = take_stereos("class A <<Entity>> <<  (S,#FF7700) Singleton >>");
        assert_eq!(rest, "class A");
        assert_eq!(st[0].text, "Entity");
        assert_eq!(st[1].spot, Some(('S', Some("#ff7700ff".into()))));
        assert_eq!(st[1].text, "Singleton");
    }

    fn note(src: &str) -> NoteSyntax {
        let s = lexer::lex(src.as_bytes()).unwrap();
        let mut cur = Cursor::new(&s.lines);
        let l = cur.next().unwrap();
        parse_note(l, &mut cur).unwrap().unwrap()
    }

    #[test]
    fn notes() {
        let n = note("note left of Foo : hello");
        assert_eq!((n.side, n.text.as_str(), n.place), (Dir::Left, "hello", NoteWhere::Of(Some(Ref::Name("Foo".into())))));
        let n = note("note right of Foo #pink\nline one\nline two\nend note");
        assert_eq!(n.text, "line one\nline two");
        assert!(n.look.fill.is_some());
        let n = note("note \"text\" as N1");
        assert_eq!((n.alias.as_deref(), n.text.as_str(), n.place), (Some("N1"), "text", NoteWhere::Floating));
        assert_eq!(note("note on link : x").place, NoteWhere::OnLink);
        assert_eq!(note("note top : x").place, NoteWhere::Of(None));
        assert_eq!(note("floating note left : x").side, Dir::Left);
        assert_eq!(note("note right of Foo::bar : x").place, NoteWhere::Of(Some(Ref::Name("Foo::bar".into()))));
        let s = lexer::lex(b"class A").unwrap();
        let mut cur = Cursor::new(&s.lines);
        let l = cur.next().unwrap();
        assert!(parse_note(l, &mut cur).unwrap().is_none());
        let s = lexer::lex(b"note left of A\nnever closed").unwrap();
        let mut cur = Cursor::new(&s.lines);
        let l = cur.next().unwrap();
        assert!(parse_note(l, &mut cur).unwrap_err().contains("never closed"));
    }

    #[test]
    fn group_styles() {
        assert_eq!(GroupStyle::from_word("Folder"), Some(GroupStyle::Folder));
        assert_eq!(GroupStyle::from_word("namespace"), Some(GroupStyle::Package));
        assert!(GroupStyle::from_word("banana").is_none());
    }
}
