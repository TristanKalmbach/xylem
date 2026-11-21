use crate::{Attr, Doctype, Document, Element, NodeId, NodeKind};
use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub struct ParseError {
    pub message: String,
    pub offset: usize,
    pub line: usize,
    pub column: usize,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "line {}, column {}: {}",
            self.line, self.column, self.message
        )
    }
}

impl std::error::Error for ParseError {}

pub fn parse(text: &str) -> Result<Document, ParseError> {
    let mut p = Parser {
        src: text,
        pos: 0,
        doc: Document::new(),
        fragment: false,
    };
    p.run()?;
    Ok(p.doc)
}

/// Parses well-balanced content (any number of top-level nodes).
pub fn parse_fragment(text: &str) -> Result<Document, ParseError> {
    let mut p = Parser {
        src: text,
        pos: 0,
        doc: Document::new(),
        fragment: true,
    };
    p.run()?;
    Ok(p.doc)
}

struct Parser<'a> {
    src: &'a str,
    pos: usize,
    doc: Document,
    fragment: bool,
}

fn is_name_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || c == ':' || (c as u32) > 0x7F
}

fn is_name_char(c: char) -> bool {
    is_name_start(c) || c.is_ascii_digit() || c == '-' || c == '.' || c == '\u{B7}'
}

impl<'a> Parser<'a> {
    fn err(&self, msg: impl Into<String>) -> ParseError {
        self.err_at(self.pos, msg)
    }

    fn err_at(&self, offset: usize, msg: impl Into<String>) -> ParseError {
        let before = &self.src[..offset.min(self.src.len())];
        let line = before.matches('\n').count() + 1;
        let column = before
            .rsplit('\n')
            .next()
            .map(|l| l.chars().count())
            .unwrap_or(0)
            + 1;
        ParseError {
            message: msg.into(),
            offset,
            line,
            column,
        }
    }

    fn rest(&self) -> &'a str {
        &self.src[self.pos..]
    }

    fn starts(&self, s: &str) -> bool {
        self.rest().starts_with(s)
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.pos += c.len_utf8();
            } else {
                break;
            }
        }
    }

    fn expect(&mut self, s: &str) -> Result<(), ParseError> {
        if self.starts(s) {
            self.pos += s.len();
            Ok(())
        } else {
            Err(self.err(format!("expected `{s}`")))
        }
    }

    fn name(&mut self) -> Result<&'a str, ParseError> {
        let start = self.pos;
        match self.peek() {
            Some(c) if is_name_start(c) => {
                self.bump();
            }
            _ => return Err(self.err("expected a name")),
        }
        while let Some(c) = self.peek() {
            if is_name_char(c) {
                self.bump();
            } else {
                break;
            }
        }
        Ok(&self.src[start..self.pos])
    }

    fn until(&mut self, end: &str, what: &str) -> Result<&'a str, ParseError> {
        let start = self.pos;
        match self.rest().find(end) {
            Some(i) => {
                self.pos += i + end.len();
                Ok(&self.src[start..start + i])
            }
            None => Err(self.err_at(start, format!("unterminated {what}"))),
        }
    }

    fn leaf(&mut self, parent: NodeId, kind: NodeKind, start: usize) {
        let id = self.doc.create_node(kind);
        self.doc.nodes[id.index()].raw = Some(self.src[start..self.pos].to_string());
        self.doc.nodes[id.index()].parent = Some(parent);
        self.doc.nodes[parent.index()].children.push(id);
    }

    fn run(&mut self) -> Result<(), ParseError> {
        let mut stack: Vec<(NodeId, &'a str, usize)> = Vec::new();
        let root = self.doc.root();
        let mut seen_root_element = false;
        while self.pos < self.src.len() {
            let parent = stack.last().map(|s| s.0).unwrap_or(root);
            let start = self.pos;
            if self.starts("<!--") {
                self.pos += 4;
                let body = self.until("-->", "comment")?;
                self.leaf(parent, NodeKind::Comment(body.to_string()), start);
            } else if self.starts("<![CDATA[") {
                if stack.is_empty() && !self.fragment {
                    return Err(self.err("CDATA outside the document element"));
                }
                self.pos += 9;
                let body = self.until("]]>", "CDATA section")?;
                self.leaf(parent, NodeKind::CData(body.to_string()), start);
            } else if self.starts("<?") {
                self.pos += 2;
                let target = self.name()?.to_string();
                let body = self.until("?>", "processing instruction")?;
                let data = body.trim_start().to_string();
                self.leaf(parent, NodeKind::Pi { target, data }, start);
            } else if self.starts("<!DOCTYPE") {
                if !stack.is_empty() {
                    return Err(self.err("DOCTYPE inside element"));
                }
                let dt = self.doctype()?;
                self.leaf(parent, NodeKind::Doctype(dt), start);
            } else if self.starts("</") {
                self.pos += 2;
                let name = self.name()?;
                self.skip_ws();
                self.expect(">")?;
                match stack.pop() {
                    Some((id, open, open_pos)) => {
                        if open != name {
                            return Err(self.err_at(
                                start,
                                format!("end tag `</{name}>` does not match start tag `<{open}>` (opened at {})", {
                                    let e = self.err_at(open_pos, "");
                                    format!("line {}", e.line)
                                }),
                            ));
                        }
                        self.doc.nodes[id.index()].raw_end =
                            Some(self.src[start..self.pos].to_string());
                    }
                    None => {
                        return Err(self.err_at(start, format!("unexpected end tag `</{name}>`")));
                    }
                }
            } else if self.starts("<") {
                if stack.is_empty() && seen_root_element && !self.fragment {
                    return Err(self.err("content after the document element"));
                }
                self.pos += 1;
                let name = self.name()?;
                let mut attrs = Vec::new();
                loop {
                    let astart = self.pos;
                    self.skip_ws();
                    if self.starts("/>") || self.starts(">") {
                        break;
                    }
                    if self.pos == astart {
                        return Err(self.err("expected whitespace before attribute"));
                    }
                    let aname = self.name()?;
                    self.skip_ws();
                    self.expect("=")?;
                    self.skip_ws();
                    let q = match self.bump() {
                        Some(q @ ('"' | '\'')) => q,
                        _ => return Err(self.err("expected quoted attribute value")),
                    };
                    let vstart = self.pos;
                    let Some(len) = self.rest().find(q) else {
                        return Err(self.err_at(vstart, "unterminated attribute value"));
                    };
                    let rawv = &self.src[vstart..vstart + len];
                    if rawv.contains('<') {
                        return Err(self.err_at(vstart, "`<` in attribute value"));
                    }
                    self.pos = vstart + len + 1;
                    let value = decode_entities(rawv, true, |off, m| self.err_at(vstart + off, m))?;
                    if attrs.iter().any(|a: &Attr| a.name == aname) {
                        return Err(self.err_at(astart, format!("duplicate attribute `{aname}`")));
                    }
                    attrs.push(Attr {
                        name: aname.to_string(),
                        value,
                        raw: Some(self.src[astart..self.pos].to_string()),
                    });
                }
                let self_close = self.starts("/>");
                self.pos += if self_close { 2 } else { 1 };
                let id = self.doc.create_node(NodeKind::Element(Element {
                    name: name.to_string(),
                    attrs,
                }));
                self.doc.nodes[id.index()].raw = Some(self.src[start..self.pos].to_string());
                self.doc.nodes[id.index()].parent = Some(parent);
                self.doc.nodes[parent.index()].children.push(id);
                if stack.is_empty() {
                    seen_root_element = true;
                }
                if !self_close {
                    stack.push((id, name, start));
                }
            } else {
                // Character data up to the next markup.
                let end = self
                    .rest()
                    .find('<')
                    .map(|i| self.pos + i)
                    .unwrap_or(self.src.len());
                let raw = &self.src[self.pos..end];
                if stack.is_empty() && !self.fragment {
                    if !raw.trim().is_empty() {
                        return Err(self.err("text outside the document element"));
                    }
                    self.pos = end;
                    self.leaf(parent, NodeKind::Text(raw.to_string()), start);
                    continue;
                }
                self.text_run(parent, raw, start)?;
                self.pos = end;
            }
        }
        if let Some((_, name, open)) = stack.last() {
            return Err(self.err_at(*open, format!("element `<{name}>` is never closed")));
        }
        if !seen_root_element && !self.fragment {
            return Err(self.err("no document element"));
        }
        Ok(())
    }

    /// Splits character data into text nodes and entity-reference nodes.
    fn text_run(&mut self, parent: NodeId, raw: &'a str, base: usize) -> Result<(), ParseError> {
        let mut seg_start = 0;
        let mut i = 0;
        let bytes = raw.as_bytes();
        while i < bytes.len() {
            if bytes[i] == b'&' {
                let semi = raw[i..]
                    .find(';')
                    .ok_or_else(|| self.err_at(base + i, "unterminated entity reference"))?;
                let name = &raw[i + 1..i + semi];
                let known =
                    name.starts_with('#') || matches!(name, "lt" | "gt" | "amp" | "apos" | "quot");
                if !known {
                    if seg_start < i {
                        self.push_text(parent, &raw[seg_start..i], base + seg_start)?;
                    }
                    let id = self.doc.create_node(NodeKind::EntityRef(name.to_string()));
                    self.doc.nodes[id.index()].raw = Some(raw[i..i + semi + 1].to_string());
                    self.doc.nodes[id.index()].parent = Some(parent);
                    self.doc.nodes[parent.index()].children.push(id);
                    seg_start = i + semi + 1;
                }
                i += semi + 1;
            } else {
                i += 1;
            }
        }
        if seg_start < raw.len() {
            self.push_text(parent, &raw[seg_start..], base + seg_start)?;
        }
        Ok(())
    }

    fn push_text(&mut self, parent: NodeId, raw: &str, base: usize) -> Result<(), ParseError> {
        let value = decode_entities(raw, false, |off, m| self.err_at(base + off, m))?;
        let id = self.doc.create_node(NodeKind::Text(value));
        self.doc.nodes[id.index()].raw = Some(raw.to_string());
        self.doc.nodes[id.index()].parent = Some(parent);
        self.doc.nodes[parent.index()].children.push(id);
        Ok(())
    }

    fn doctype(&mut self) -> Result<Doctype, ParseError> {
        self.expect("<!DOCTYPE")?;
        self.skip_ws();
        let mut dt = Doctype {
            name: self.name()?.to_string(),
            ..Default::default()
        };
        self.skip_ws();
        let quoted = |p: &mut Self| -> Result<String, ParseError> {
            p.skip_ws();
            let q = match p.bump() {
                Some(q @ ('"' | '\'')) => q,
                _ => return Err(p.err("expected quoted literal")),
            };
            let s = p.until(&q.to_string(), "literal")?;
            Ok(s.to_string())
        };
        if self.starts("PUBLIC") {
            self.pos += 6;
            dt.public_id = Some(quoted(self)?);
            self.skip_ws();
            if matches!(self.peek(), Some('"' | '\'')) {
                dt.system_id = Some(quoted(self)?);
            }
        } else if self.starts("SYSTEM") {
            self.pos += 6;
            dt.system_id = Some(quoted(self)?);
        }
        self.skip_ws();
        if self.starts("[") {
            self.pos += 1;
            let start = self.pos;
            // Scan the internal subset honouring quotes and comments.
            loop {
                if self.pos >= self.src.len() {
                    return Err(self.err_at(start, "unterminated internal subset"));
                }
                if self.starts("<!--") {
                    self.pos += 4;
                    self.until("-->", "comment")?;
                    continue;
                }
                match self.peek() {
                    Some(']') => break,
                    Some(q @ ('"' | '\'')) => {
                        self.bump();
                        self.until(&q.to_string(), "literal")?;
                    }
                    _ => {
                        self.bump();
                    }
                }
            }
            dt.internal_subset = Some(self.src[start..self.pos].to_string());
            self.pos += 1;
            self.skip_ws();
        }
        self.expect(">")?;
        Ok(dt)
    }
}

/// Expands predefined and character references.
fn decode_entities(
    raw: &str,
    attr: bool,
    err: impl Fn(usize, String) -> ParseError,
) -> Result<String, ParseError> {
    if !raw.contains('&') && !(attr && raw.contains(['\n', '\t', '\r'])) {
        return Ok(raw.to_string());
    }
    let mut out = String::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        let c = raw[i..].chars().next().unwrap();
        if c == '&' {
            let semi = raw[i..]
                .find(';')
                .ok_or_else(|| err(i, "unterminated entity reference".into()))?;
            let name = &raw[i + 1..i + semi];
            let ch = match name {
                "lt" => Some('<'),
                "gt" => Some('>'),
                "amp" => Some('&'),
                "apos" => Some('\''),
                "quot" => Some('"'),
                n if n.starts_with("#x") || n.starts_with("#X") => u32::from_str_radix(&n[2..], 16)
                    .ok()
                    .and_then(char::from_u32),
                n if n.starts_with('#') => n[1..].parse::<u32>().ok().and_then(char::from_u32),
                _ => None,
            };
            match ch {
                Some(ch) => out.push(ch),
                None if attr => {
                    // Unknown entity in attribute: keep literally.
                    out.push_str(&raw[i..i + semi + 1]);
                }
                None => return Err(err(i, format!("unknown entity `&{name};`"))),
            }
            i += semi + 1;
        } else {
            if attr && matches!(c, '\n' | '\t' | '\r') {
                out.push(' ');
            } else {
                out.push(c);
            }
            i += c.len_utf8();
        }
    }
    Ok(out)
}
