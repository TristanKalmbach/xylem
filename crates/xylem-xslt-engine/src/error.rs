//! Errors with a location, for editors and embedding applications.
//!
//! Inside the engine an error is a `String`. Errors that know where they
//! happened start with `file:line: ` or `file:line:column: ` (written by
//! [`located`]), and each template or function an error passes through adds
//! a line `  in template name (file:line)` or `  in function name() (file:line)`
//! (written by [`frame`]). [`XsltError::from_message`] reads that text back
//! into its parts, so the public API can offer both the text and the
//! structure.

use std::fmt;

/// One template or function an error passed through, innermost first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    /// `template` or `function`.
    pub kind: String,
    /// The template's name or pattern (`file.xsl#name`), or the function's
    /// name.
    pub name: String,
    /// The module the declaration is in.
    pub file: String,
    /// The line of the declaration's start tag.
    pub line: u32,
}

/// A compile or run-time error: its message and, where known, the
/// stylesheet file, line and column it refers to, and the templates and
/// functions it passed through.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XsltError {
    pub message: String,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
    /// Innermost first.
    pub stack: Vec<Frame>,
}

/// Prefixes `msg` with its location, in the form [`XsltError`] reads back.
pub fn located(file: &str, line: u32, column: Option<u32>, msg: &str) -> String {
    let file = file.strip_prefix(r"\\?\").unwrap_or(file);
    match column {
        Some(c) => format!("{file}:{line}:{c}: {msg}"),
        None => format!("{file}:{line}: {msg}"),
    }
}

/// The line a template or function adds to an error passing through it.
pub fn frame(kind: &str, name: &str, file: &str, line: u32) -> String {
    let file = file.strip_prefix(r"\\?\").unwrap_or(file);
    format!("\n  in {kind} {name} ({file}:{line})")
}

/// Splits `file:line[:column]: rest` at its location, if it has one. File
/// names may contain colons (`C:\a.xsl`), so the location is the first
/// `:digits:` that is followed by a space or by `digits: `.
fn split_location(s: &str) -> Option<(&str, u32, Option<u32>, &str)> {
    let b = s.as_bytes();
    let digits = |from: usize| {
        let mut j = from;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        (j > from).then_some(j)
    };
    let mut i = 0;
    while let Some(off) = s[i..].find(':') {
        let colon = i + off;
        if let Some(end) = digits(colon + 1)
            && b.get(end) == Some(&b':')
        {
            let line: u32 = s[colon + 1..end].parse().ok()?;
            if b.get(end + 1) == Some(&b' ') {
                return Some((&s[..colon], line, None, &s[end + 2..]));
            }
            if let Some(cend) = digits(end + 1)
                && s[cend..].starts_with(": ")
            {
                let col = s[end + 1..cend].parse().ok();
                return Some((&s[..colon], line, col, &s[cend + 2..]));
            }
        }
        i = colon + 1;
    }
    None
}

/// Reads `  in template name (file:line)` back.
fn parse_frame(line: &str) -> Option<Frame> {
    let rest = line.trim_start().strip_prefix("in ")?;
    let (kind, rest) = rest.split_once(' ')?;
    let open = rest.rfind(" (")?;
    let inner = rest[open + 2..].strip_suffix(')')?;
    let colon = inner.rfind(':')?;
    Some(Frame {
        kind: kind.to_string(),
        name: rest[..open].to_string(),
        file: inner[..colon].to_string(),
        line: inner[colon + 1..].parse().ok()?,
    })
}

impl XsltError {
    /// Reads an engine error message into its parts. A message without a
    /// location of its own takes the innermost frame's.
    pub fn from_message(text: &str) -> XsltError {
        let mut lines = text.lines();
        let first = lines.next().unwrap_or("");
        let mut message_lines = vec![];
        let mut stack = Vec::new();
        for l in lines {
            match parse_frame(l) {
                Some(f) => stack.push(f),
                None if stack.is_empty() => message_lines.push(l),
                None => {}
            }
        }
        let (file, line, column, head) = match split_location(first) {
            Some((f, l, c, rest)) => (Some(f.to_string()), Some(l), c, rest.to_string()),
            None => match stack.first() {
                Some(f) => (Some(f.file.clone()), Some(f.line), None, first.to_string()),
                None => (None, None, None, first.to_string()),
            },
        };
        let mut message = head;
        for l in message_lines {
            message.push('\n');
            message.push_str(l);
        }
        XsltError {
            message,
            file,
            line,
            column,
            stack,
        }
    }

    /// The error as JSON: `{"message", "file", "line", "column", "stack":
    /// [{"kind", "name", "file", "line"}]}`, absent parts as `null`.
    pub fn to_json(&self) -> String {
        fn s(v: &str) -> String {
            let mut o = String::from("\"");
            for c in v.chars() {
                match c {
                    '"' => o.push_str("\\\""),
                    '\\' => o.push_str("\\\\"),
                    '\n' => o.push_str("\\n"),
                    '\r' => o.push_str("\\r"),
                    '\t' => o.push_str("\\t"),
                    c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
                    c => o.push(c),
                }
            }
            o.push('"');
            o
        }
        let opt = |v: Option<u32>| v.map_or("null".to_string(), |n| n.to_string());
        let stack: Vec<String> = self
            .stack
            .iter()
            .map(|f| {
                format!(
                    r#"{{"kind":{},"name":{},"file":{},"line":{}}}"#,
                    s(&f.kind),
                    s(&f.name),
                    s(&f.file),
                    f.line
                )
            })
            .collect();
        format!(
            r#"{{"message":{},"file":{},"line":{},"column":{},"stack":[{}]}}"#,
            s(&self.message),
            self.file.as_deref().map_or("null".to_string(), s),
            opt(self.line),
            opt(self.column),
            stack.join(",")
        )
    }
}

impl fmt::Display for XsltError {
    /// The engine's text form, which [`XsltError::from_message`] reads.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let head = match (&self.file, self.line) {
            (Some(file), Some(line))
                if self
                    .stack
                    .first()
                    .is_none_or(|fr| fr.file != *file || fr.line != line) =>
            {
                located(file, line, self.column, &self.message)
            }
            _ => self.message.clone(),
        };
        f.write_str(&head)?;
        for fr in &self.stack {
            f.write_str(&frame(&fr.kind, &fr.name, &fr.file, fr.line))?;
        }
        Ok(())
    }
}

impl std::error::Error for XsltError {}

impl From<XsltError> for String {
    fn from(e: XsltError) -> String {
        e.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locations_and_frames_round_trip() {
        let text = format!(
            "{}{}{}",
            located(r"C:\s\a b.xsl", 12, Some(7), "XPST0008: no variable $x"),
            frame("template", "a b.xsl#title", r"C:\s\a b.xsl", 10),
            frame("function", "f:g()", "/s/f.xsl", 3)
        );
        let e = XsltError::from_message(&text);
        assert_eq!(e.message, "XPST0008: no variable $x");
        assert_eq!(e.file.as_deref(), Some(r"C:\s\a b.xsl"));
        assert_eq!((e.line, e.column), (Some(12), Some(7)));
        assert_eq!(e.stack.len(), 2);
        assert_eq!(e.stack[1].name, "f:g()");
        assert_eq!(e.stack[1].line, 3);
        assert_eq!(e.to_string(), text);
        assert!(e.to_json().contains(r#""line":12,"column":7"#));
    }

    #[test]
    fn a_runtime_error_takes_its_innermost_frame() {
        let text = format!(
            "division by zero{}",
            frame("template", "x.xsl#/", "/s/x.xsl", 4)
        );
        let e = XsltError::from_message(&text);
        assert_eq!(e.message, "division by zero");
        assert_eq!((e.file.as_deref(), e.line), (Some("/s/x.xsl"), Some(4)));
        assert_eq!(e.to_string(), text);
        let plain = XsltError::from_message("just a message");
        assert_eq!((plain.file, plain.line), (None, None));
    }
}
