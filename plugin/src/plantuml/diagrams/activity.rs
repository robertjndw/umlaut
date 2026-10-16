//! Activity diagrams (the "new" syntax): statements to a tree of nested blocks.
//!
//! Every statement that draws something becomes an `Item` in the block being filled;
//! constructs with bodies (`if`, `while`, `fork`, ...) own sub-blocks. The lane that
//! was active when an item was created is stored with it, so the layout can place
//! items from different swimlanes in their columns even in the middle of a branch.

use crate::plantuml::colors::{parse_color, parse_paint};
use crate::plantuml::creole;
use crate::plantuml::lexer::{ends_action, err, first_word, is_end, rest_after_word, unquote, Cursor, Line};
use crate::plantuml::model::{Ctx, LabelId};
use crate::scene::Paint;

/// Look of the arrow leading into an item.
#[derive(Debug, Clone, Default)]
pub struct ArrowSpec {
    pub label: Option<LabelId>,
    pub color: Option<String>,
    pub dashed: bool,
    pub bold: bool,
    pub hidden: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionShape {
    /// `:text;`
    Plain,
    /// `:text|` procedure, drawn with double side bars.
    Procedure,
    /// `:text<` input, a flag pointing left.
    Input,
    /// `:text>` output, a flag pointing right.
    Output,
    /// `:text/` slanted right edge.
    Slant,
    /// `:text]` concave right edge.
    Concave,
    /// `:text}` convex right edge.
    Convex,
}

#[derive(Debug, Clone)]
pub struct Note {
    pub right: bool,
    pub label: LabelId,
    pub fill: Option<Paint>,
}

#[derive(Debug, Clone)]
pub struct IfBranch {
    pub cond: LabelId,
    pub label: Option<LabelId>,
    pub body: Block,
}

#[derive(Debug, Clone)]
pub struct Case {
    pub label: Option<LabelId>,
    pub body: Block,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupKind {
    Partition,
    Package,
    Rectangle,
    Group,
    Card,
}

#[derive(Debug, Clone)]
pub enum Node {
    Start,
    Stop,
    /// `end`, drawn like `stop`.
    End,
    Kill,
    Detach,
    Break,
    Action { label: LabelId, shape: ActionShape, fill: Option<Paint> },
    Connector { label: LabelId },
    If { branches: Vec<IfBranch>, else_: Option<(Option<LabelId>, Block)>, end_lane: usize },
    Switch { cond: LabelId, cases: Vec<Case>, end_lane: usize },
    While { cond: LabelId, is_label: Option<LabelId>, end_label: Option<LabelId>, body: Block, end_lane: usize },
    Repeat { body: Block, cond: Option<LabelId>, is_label: Option<LabelId>, not_label: Option<LabelId>, backward: Option<Box<Item>>, end_lane: usize },
    Fork { branches: Vec<Block>, merge: bool, end_lane: usize },
    Split { branches: Vec<Block>, end_lane: usize },
    Group { kind: GroupKind, title: Option<LabelId>, fill: Option<Paint>, body: Block },
}

#[derive(Debug, Clone)]
pub struct Item {
    pub node: Node,
    pub lane: usize,
    pub arrow: Option<ArrowSpec>,
    pub notes: Vec<Note>,
    pub line: usize,
}

pub type Block = Vec<Item>;

#[derive(Debug, Clone)]
pub struct Lane {
    pub label: LabelId,
    pub fill: Option<Paint>,
}

#[derive(Debug)]
pub struct Model {
    pub body: Block,
    pub lanes: Vec<Lane>,
}

struct P<'a> {
    ctx: &'a mut Ctx,
    lanes: Vec<Lane>,
    lane_names: Vec<String>,
    lane: usize,
    arrow: Option<ArrowSpec>,
    notes: Vec<Note>,
    /// Blocks currently being parsed, see `lexer::MAX_NESTING`.
    depth: usize,
}

/// Lowercased line with `{...}` suffixes and extra spaces dropped, for matching keywords.
fn norm(text: &str) -> String {
    let t = text.split('{').next().unwrap_or("");
    t.split_whitespace().collect::<Vec<_>>().join(" ").to_ascii_lowercase()
}

/// `(inner)` at the start of `s`, with nested parentheses balanced; the rest follows.
fn paren(s: &str) -> Option<(&str, &str)> {
    let s = s.trim_start();
    let inner = s.strip_prefix('(')?;
    let mut depth = 1;
    for (i, c) in inner.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some((&inner[..i], &inner[i + 1..]));
                }
            }
            _ => {}
        }
    }
    None
}

pub fn parse(lines: &[Line], ctx: &mut Ctx) -> Result<Model, String> {
    let mut p = P { ctx, lanes: Vec::new(), lane_names: Vec::new(), lane: 0, arrow: None, notes: Vec::new(), depth: 0 };
    let mut cur = Cursor::new(lines);
    let (body, stop) = p.block(&mut cur, &|_| false)?;
    if let Some(s) = stop {
        return err(s.no, format!("unexpected `{}`", s.text));
    }
    if body.is_empty() {
        return err(lines.first().map_or(1, |l| l.no), "the activity diagram is empty");
    }
    Ok(Model { body, lanes: p.lanes })
}

impl P<'_> {
    fn text_base(&self) -> creole::Base {
        self.ctx.skin.text(&["activity"], 12.0, "#000000")
    }

    fn label(&mut self, text: &str) -> LabelId {
        let runs = creole::runs(text, &self.text_base());
        self.ctx.reg.add(runs, None)
    }

    fn label_opt(&mut self, text: &str) -> Option<LabelId> {
        let runs = creole::runs(text, &self.text_base());
        self.ctx.reg.add_opt(runs, None)
    }

    /// Smaller text on arrows and next to branches.
    fn small(&mut self, text: &str) -> Option<LabelId> {
        let base = self.ctx.skin.text(&["activityarrow", "arrow"], 11.0, "#000000");
        let runs = creole::runs(text, &base);
        self.ctx.reg.add_opt(runs, None)
    }

    fn push(&mut self, block: &mut Block, node: Node, line: usize) {
        let head = self.head();
        self.push_with(block, node, line, head);
    }

    /// Lane and incoming arrow of a construct, taken before its body is parsed so the
    /// first item inside does not claim them.
    fn head(&mut self) -> (usize, Option<ArrowSpec>) {
        (self.lane, self.arrow.take())
    }

    fn push_with(&mut self, block: &mut Block, node: Node, line: usize, head: (usize, Option<ArrowSpec>)) {
        let notes = std::mem::take(&mut self.notes);
        block.push(Item { node, lane: head.0, arrow: head.1, notes, line });
    }

    /// Parses statements into `block` until `stop` accepts a line (which is consumed and
    /// returned) or the lines run out.
    fn block<'l>(&mut self, cur: &mut Cursor<'l>, stop: &dyn Fn(&str) -> bool) -> Result<(Block, Option<&'l Line>), String> {
        crate::plantuml::lexer::check_nesting(self.depth, cur.peek().map_or(0, |l| l.no))?;
        self.depth += 1;
        let result = self.block_lines(cur, stop);
        self.depth -= 1;
        result
    }

    fn block_lines<'l>(&mut self, cur: &mut Cursor<'l>, stop: &dyn Fn(&str) -> bool) -> Result<(Block, Option<&'l Line>), String> {
        let mut block = Block::new();
        while let Some(l) = cur.next() {
            if l.text.is_empty() {
                continue;
            }
            if stop(&norm(&l.text)) {
                return Ok((block, Some(l)));
            }
            self.statement(l, cur, &mut block)?;
        }
        Ok((block, None))
    }

    fn closed<'l>(&mut self, cur: &mut Cursor<'l>, open: &Line, stop: &dyn Fn(&str) -> bool, what: &str) -> Result<(Block, &'l Line), String> {
        let (b, s) = self.block(cur, stop)?;
        match s {
            Some(s) => Ok((b, s)),
            None => err(open.no, format!("{what} is never closed")),
        }
    }

    fn statement(&mut self, l: &Line, cur: &mut Cursor, block: &mut Block) -> Result<(), String> {
        let text = l.text.as_str();
        let n = norm(text);
        let word = n.split(' ').next().unwrap_or("");

        if let Some(first) = text.strip_prefix(':') {
            return self.action(l, cur, block, None, first);
        }
        if text.starts_with('#') {
            if let Some(colon) = text.find(':') {
                let fill = parse_paint(&text[..colon]);
                if fill.is_some() {
                    return self.action(l, cur, block, fill, &text[colon + 1..]);
                }
            }
        }
        if text.starts_with('|') {
            return self.swimlane(l);
        }
        if text.starts_with('(') && text.ends_with(')') {
            let inner = &text[1..text.len() - 1];
            let label = self.label(inner);
            self.push(block, Node::Connector { label }, l.no);
            return Ok(());
        }
        if text.starts_with('-') && (text.ends_with(';') || text.ends_with('>')) && text.contains('>') {
            return self.arrow(l);
        }
        match n.as_str() {
            "start" => {
                self.push(block, Node::Start, l.no);
                return Ok(());
            }
            "stop" => {
                self.push(block, Node::Stop, l.no);
                return Ok(());
            }
            "end" => {
                self.push(block, Node::End, l.no);
                return Ok(());
            }
            "kill" => {
                self.push(block, Node::Kill, l.no);
                return Ok(());
            }
            "detach" => {
                self.push(block, Node::Detach, l.no);
                return Ok(());
            }
            "break" => {
                self.push(block, Node::Break, l.no);
                return Ok(());
            }
            "fork" => return self.fork(l, cur, block),
            "split" => return self.split(l, cur, block),
            "repeat" => return self.repeat(l, cur, block, ""),
            "left to right direction" | "top to bottom direction" => {
                self.ctx.warn(l.no, "the layout direction is not supported in activity diagrams and is ignored");
                return Ok(());
            }
            _ => {}
        }
        let rest = rest_after_word(text);
        match word {
            "if" => self.if_(l, cur, block, rest),
            "while" => self.while_(l, cur, block, rest),
            "switch" => self.switch(l, cur, block, rest),
            "repeat" => self.repeat(l, cur, block, rest),
            "note" | "floating" => self.note(l, cur, block, text),
            "partition" | "package" | "rectangle" | "group" | "card" => self.group(l, cur, block, word, rest),
            "label" | "goto" => err(l.no, format!("`{word}` is not supported")),
            "else" | "elseif" | "endif" | "endwhile" | "endswitch" | "case" | "again" => err(l.no, format!("`{}` without a matching opening statement", text)),
            _ if n.starts_with("end ") || n == "}" => err(l.no, format!("`{text}` without a matching opening statement")),
            _ => err(l.no, format!("syntax error, cannot understand \"{text}\"")),
        }
    }

    fn action(&mut self, l: &Line, cur: &mut Cursor, block: &mut Block, fill: Option<Paint>, first: &str) -> Result<(), String> {
        let mut lines: Vec<String> = Vec::new();
        let mut piece = first.to_string();
        let shape = loop {
            if ends_action(&piece) {
                let last = piece.pop().unwrap();
                lines.push(piece);
                break match last {
                    '|' => ActionShape::Procedure,
                    '<' => ActionShape::Input,
                    '>' => ActionShape::Output,
                    '/' => ActionShape::Slant,
                    ']' => ActionShape::Concave,
                    '}' => ActionShape::Convex,
                    _ => ActionShape::Plain,
                };
            }
            lines.push(piece);
            match cur.next() {
                Some(next) => piece = next.text.clone(),
                None => return err(l.no, "this action is missing its closing `;`"),
            }
        };
        let text = lines.join("\n");
        let label = self.label(text.trim());
        self.push(block, Node::Action { label, shape, fill }, l.no);
        Ok(())
    }

    fn arrow(&mut self, l: &Line) -> Result<(), String> {
        let t = l.text.trim_end_matches(';').trim();
        let mut spec = ArrowSpec::default();
        let mut rest = &t[1..];
        if let Some(r) = rest.strip_prefix('[') {
            let Some(end) = r.find(']') else { return err(l.no, "unterminated arrow style") };
            for part in r[..end].split(',') {
                let p = part.trim();
                match p.to_ascii_lowercase().as_str() {
                    "dashed" | "dotted" => spec.dashed = true,
                    "bold" => spec.bold = true,
                    "hidden" => spec.hidden = true,
                    "plain" => {}
                    _ => {
                        if let Some(c) = parse_color(p) {
                            spec.color = Some(c);
                        }
                    }
                }
            }
            rest = &r[end + 1..];
        }
        let rest = rest.trim_start_matches('-').trim_start_matches('>').trim();
        spec.label = self.small(rest);
        self.arrow = Some(spec);
        Ok(())
    }

    fn swimlane(&mut self, l: &Line) -> Result<(), String> {
        // `|Lane|`, `|#color|Lane|`
        let inner = l.text.trim_matches('|');
        let (color, name) = match inner.strip_prefix('#').and_then(|r| r.split_once('|')) {
            Some((c, n)) => (parse_paint(&format!("#{c}")), n),
            None => (None, inner),
        };
        let name = unquote(name.trim()).to_string();
        if name.is_empty() {
            return err(l.no, "the swimlane needs a name");
        }
        let idx = match self.lane_names.iter().position(|n| *n == name) {
            Some(i) => i,
            None => {
                let mut base = self.ctx.skin.text(&["swimlanetitle"], 14.0, "#000000");
                base.bold = false;
                let label = self.ctx.reg.add(creole::runs(&name, &base), None);
                self.lanes.push(Lane { label, fill: color.clone() });
                self.lane_names.push(name);
                self.lanes.len() - 1
            }
        };
        if color.is_some() {
            self.lanes[idx].fill = color;
        }
        self.lane = idx;
        Ok(())
    }

    fn note(&mut self, l: &Line, cur: &mut Cursor, block: &mut Block, text: &str) -> Result<(), String> {
        let t = text.strip_prefix("floating").map_or(text, |r| r.trim_start());
        let mut rest = rest_after_word(t);
        let word_len = rest.find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(rest.len());
        let side = rest[..word_len].to_ascii_lowercase();
        if side != "left" && side != "right" {
            return err(l.no, format!("cannot understand the note \"{text}\""));
        }
        rest = rest[word_len..].trim();
        let (fill, rest) = match rest.strip_prefix('#') {
            Some(r) => {
                let end = r.find(|c: char| c.is_whitespace() || c == ':').unwrap_or(r.len());
                (parse_paint(&r[..end]), r[end..].trim())
            }
            None => (None, rest),
        };
        let body = if let Some(b) = rest.strip_prefix(':') {
            b.trim().to_string()
        } else if rest.is_empty() {
            let lines = cur.block(l.no, "this note", |t| is_end(t, &["note"]))?;
            lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n")
        } else {
            return err(l.no, format!("cannot understand \"{rest}\" in the note"));
        };
        let base = self.ctx.skin.text(&["note"], 12.0, "#000000");
        let Some(label) = self.ctx.reg.add_opt(creole::runs(&body, &base), None) else { return err(l.no, "the note is empty") };
        let note = Note { right: side == "right", label, fill };
        match block.last_mut() {
            Some(item) => item.notes.push(note),
            None => self.notes.push(note),
        }
        Ok(())
    }

    fn cond_label(&mut self, l: &Line, rest: &str) -> Result<(LabelId, String), String> {
        let Some((cond, after)) = paren(rest) else { return err(l.no, "expected the condition in parentheses, like `(is it done?)`") };
        Ok((self.label_opt(cond).unwrap_or_else(|| self.ctx.reg.add(vec![], None)), after.trim().to_string()))
    }

    /// `then (label)` or `is (label)` at the start of `s`.
    fn branch_label(&mut self, s: &str, keywords: &[&str]) -> (Option<LabelId>, String) {
        let s = s.trim();
        let w = first_word(s).to_ascii_lowercase();
        if keywords.contains(&w.as_str()) {
            let after = rest_after_word(s);
            if let Some((lbl, rest)) = paren(after) {
                return (self.small(lbl), rest.trim().to_string());
            }
            return (None, after.to_string());
        }
        (None, s.to_string())
    }

    fn if_(&mut self, l: &Line, cur: &mut Cursor, block: &mut Block, rest: &str) -> Result<(), String> {
        let head = self.head();
        let (cond, after) = self.cond_label(l, rest)?;
        let (label, _) = self.branch_label(&after, &["then", "is"]);
        let mut branches = Vec::new();
        let mut else_ = None;
        let (mut cond, mut label) = (cond, label);
        let stop = |t: &str| t.starts_with("elseif") || t == "else" || t.starts_with("else ") || t == "endif" || t == "end if";
        loop {
            let (body, s) = self.closed(cur, l, &stop, "this `if`")?;
            branches.push(IfBranch { cond, label, body });
            let sn = norm(&s.text);
            if sn == "endif" || sn == "end if" {
                break;
            }
            if sn.starts_with("elseif") {
                let rest = rest_after_word(&s.text);
                let (c, after) = self.cond_label(s, rest)?;
                let (lb, _) = self.branch_label(&after, &["then", "is"]);
                cond = c;
                label = lb;
                continue;
            }
            // else
            let lbl = paren(rest_after_word(&s.text)).and_then(|(t, _)| self.small(t));
            let (body, _) = self.closed(cur, s, &|t| t == "endif" || t == "end if", "this `else`")?;
            else_ = Some((lbl, body));
            break;
        }
        let end_lane = self.lane;
        self.push_with(block, Node::If { branches, else_, end_lane }, l.no, head);
        Ok(())
    }

    fn while_(&mut self, l: &Line, cur: &mut Cursor, block: &mut Block, rest: &str) -> Result<(), String> {
        let head = self.head();
        let (cond, after) = self.cond_label(l, rest)?;
        let (is_label, _) = self.branch_label(&after, &["is", "then"]);
        let (body, s) = self.closed(cur, l, &|t| t.starts_with("endwhile") || t == "end while", "this `while`")?;
        let end_label = paren(rest_after_word(&s.text)).and_then(|(t, _)| self.small(t));
        let end_lane = self.lane;
        self.push_with(block, Node::While { cond, is_label, end_label, body, end_lane }, l.no, head);
        Ok(())
    }

    fn switch(&mut self, l: &Line, cur: &mut Cursor, block: &mut Block, rest: &str) -> Result<(), String> {
        let head = self.head();
        let (cond, _) = self.cond_label(l, rest)?;
        // Anything before the first `case` is not allowed.
        let (junk, mut stop) = self.block(cur, &|t| t.starts_with("case") || t == "endswitch" || t == "end switch")?;
        if let Some(i) = junk.first() {
            return err(i.line, "a `switch` has to start with a `case`");
        }
        let mut cases = Vec::new();
        while let Some(s) = stop {
            let sn = norm(&s.text);
            if sn == "endswitch" || sn == "end switch" {
                let end_lane = self.lane;
                self.push_with(block, Node::Switch { cond, cases, end_lane }, l.no, head);
                return Ok(());
            }
            let label = paren(rest_after_word(&s.text)).and_then(|(t, _)| self.small(t));
            let (body, next) = self.block(cur, &|t| t.starts_with("case") || t == "endswitch" || t == "end switch")?;
            cases.push(Case { label, body });
            stop = next;
        }
        err(l.no, "this `switch` is never closed")
    }

    fn repeat(&mut self, l: &Line, cur: &mut Cursor, block: &mut Block, rest: &str) -> Result<(), String> {
        let head = self.head();
        let mut body = Block::new();
        // `repeat :first action;` on one line.
        if let Some(first) = rest.strip_prefix(':') {
            self.action(l, cur, &mut body, None, first)?;
        }
        let mut backward = None;
        let mut stop;
        loop {
            let (more, s) = self.block(cur, &|t| t.starts_with("repeat while") || t.starts_with("backward") || t == "repeat while")?;
            body.extend(more);
            match s {
                Some(s) if norm(&s.text).starts_with("backward") => {
                    let mut tmp = Block::new();
                    let after = s.text.trim_start()[8..].trim_start().trim_start_matches(':').trim_start();
                    self.action(s, cur, &mut tmp, None, after)?;
                    backward = tmp.pop().map(Box::new);
                }
                other => {
                    stop = other;
                    break;
                }
            }
        }
        let Some(s) = stop.take() else { return err(l.no, "this `repeat` is never closed with `repeat while`") };
        let rest = rest_after_word(rest_after_word(&s.text));
        let (cond, after) = match paren(rest) {
            Some((c, a)) => (self.label_opt(c), a.trim().to_string()),
            None => (None, rest.to_string()),
        };
        let (is_label, after) = self.branch_label(&after, &["is", "then"]);
        let (not_label, _) = self.branch_label(&after, &["not", "else"]);
        let end_lane = self.lane;
        self.push_with(block, Node::Repeat { body, cond, is_label, not_label, backward, end_lane }, l.no, head);
        Ok(())
    }

    fn fork(&mut self, l: &Line, cur: &mut Cursor, block: &mut Block) -> Result<(), String> {
        let head = self.head();
        let mut branches = Vec::new();
        loop {
            let (body, s) = self.closed(cur, l, &|t| t == "fork again" || t == "end fork" || t == "end merge" || t == "endfork", "this `fork`")?;
            branches.push(body);
            let sn = norm(&s.text);
            if sn != "fork again" {
                let end_lane = self.lane;
                self.push_with(block, Node::Fork { branches, merge: sn == "end merge", end_lane }, l.no, head);
                return Ok(());
            }
        }
    }

    fn split(&mut self, l: &Line, cur: &mut Cursor, block: &mut Block) -> Result<(), String> {
        let head = self.head();
        let mut branches = Vec::new();
        loop {
            let (body, s) = self.closed(cur, l, &|t| t == "split again" || t == "end split", "this `split`")?;
            branches.push(body);
            if norm(&s.text) == "end split" {
                let end_lane = self.lane;
                self.push_with(block, Node::Split { branches, end_lane }, l.no, head);
                return Ok(());
            }
        }
    }

    fn group(&mut self, l: &Line, cur: &mut Cursor, block: &mut Block, word: &str, rest: &str) -> Result<(), String> {
        let hd = self.head();
        let Some(head) = rest.trim().strip_suffix('{') else { return err(l.no, format!("`{word}` needs an opening `{{` on the same line")) };
        let mut head = head.trim();
        let mut fill = None;
        if let Some(pos) = head.rfind('#') {
            if !head[pos..].contains('"') {
                fill = parse_paint(head[pos..].trim());
                head = head[..pos].trim();
            }
        }
        let name = unquote(head);
        let mut base = self.ctx.skin.text(&["partition", "package"], 12.0, "#000000");
        base.bold = false;
        let title = if name.is_empty() { None } else { self.ctx.reg.add_opt(creole::runs(name, &base), None) };
        let (body, _) = self.closed(cur, l, &|t| t == "}", "this group")?;
        let kind = match word {
            "partition" => GroupKind::Partition,
            "package" => GroupKind::Package,
            "rectangle" => GroupKind::Rectangle,
            "card" => GroupKind::Card,
            _ => GroupKind::Group,
        };
        self.push_with(block, Node::Group { kind, title, fill, body }, l.no, hd);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(src: &str) -> Result<(Model, Ctx), String> {
        crate::plantuml::parse_with(src, parse)
    }

    #[test]
    fn simple_flow() {
        let (m, ctx) = model("start\n:Hello;\n:multi\nline;\nstop").unwrap();
        assert_eq!(m.body.len(), 4);
        assert!(matches!(m.body[0].node, Node::Start));
        let Node::Action { label, .. } = &m.body[2].node else { panic!() };
        assert_eq!(creole::plain(&ctx.reg.labels[label.0].runs), "multi\nline");
    }

    #[test]
    fn if_chain() {
        let (m, _) = model("if (a?) then (yes)\n:x;\nelseif (b?) then (maybe)\n:y;\nelse (no)\n:z;\nendif").unwrap();
        let Node::If { branches, else_, .. } = &m.body[0].node else { panic!() };
        assert_eq!(branches.len(), 2);
        assert!(else_.is_some());
    }

    #[test]
    fn loops_forks_groups() {
        let (m, _) = model("while (x) is (yes)\n:a;\nendwhile (no)\nrepeat\n:b;\nbackward :c;\nrepeat while (d?) is (y) not (n)\nfork\n:e;\nfork again\n:f;\nend fork\nsplit\n:g;\nsplit again\n:h;\nend split\npartition P {\n:i;\n}").unwrap();
        assert_eq!(m.body.len(), 5);
        let Node::Repeat { backward, .. } = &m.body[1].node else { panic!() };
        assert!(backward.is_some());
        let Node::Fork { branches, .. } = &m.body[2].node else { panic!() };
        assert_eq!(branches.len(), 2);
    }

    #[test]
    fn swimlanes_and_arrows() {
        let (m, _) = model("|A|\nstart\n-> go;\n|#pink|B|\n:x;\n-[#red,dashed]->\n:y;").unwrap();
        assert_eq!(m.lanes.len(), 2);
        assert_eq!(m.body[1].lane, 1);
        assert!(m.body[1].arrow.as_ref().unwrap().label.is_some());
        assert!(m.body[2].arrow.as_ref().unwrap().dashed);
    }

    #[test]
    fn notes_attach_to_the_previous_item() {
        let (m, _) = model(":a;\nnote right: hi\n:b;\nnote left\nlong\nend note").unwrap();
        assert_eq!(m.body[0].notes.len(), 1);
        assert!(!m.body[1].notes[0].right);
    }

    #[test]
    fn errors_have_line_numbers() {
        assert!(model("start\nif (x) then\n:a;").unwrap_err().starts_with("line 2:"));
        assert!(model("start\nbogus words").unwrap_err().starts_with("line 2:"));
        assert!(model("start\nendif").unwrap_err().starts_with("line 2:"));
        assert!(model(":never closed").unwrap_err().starts_with("line 1:"));
    }
}
