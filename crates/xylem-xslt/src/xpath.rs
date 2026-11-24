//! A small XPath 2.0 reader for analysing stylesheets: splits an expression
//! into tokens with their exact positions, and recognises the few shapes the
//! analyser looks for (simple paths, lookups, context-free arguments).
//!
//! It is not an evaluator. Anything it does not recognise is left alone:
//! the analyser only proposes a fix for expressions it fully understands.

use std::ops::Range;

/// What a token is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A name: element or attribute name, function name, keyword, axis.
    Name,
    /// `$name`.
    Variable,
    /// A quoted string literal.
    String,
    Number,
    /// `*` used as a name test or as multiplication.
    Star,
    /// `/` and `//`.
    Slash,
    DoubleSlash,
    /// `[` `]` `(` `)` `,` `@` `::` `.` `..`
    LBracket,
    RBracket,
    LParen,
    RParen,
    Comma,
    At,
    Axis,
    Dot,
    DotDot,
    /// `=` `!=` `<` `<=` `>` `>=` `|` `+` `-` `?` `:=` `:` and the like.
    Op,
}

/// A token and where it is in the expression (byte offsets).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: Kind,
    pub span: Range<usize>,
}

impl Token {
    pub fn text<'a>(&self, src: &'a str) -> &'a str {
        &src[self.span.clone()]
    }
}

fn is_name_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '\u{b7}')
}

/// Splits `src` into tokens. Comments `(: … :)` and whitespace are skipped.
/// Returns `None` for text it cannot read (an unterminated string, say).
pub fn tokenize(src: &str) -> Option<Vec<Token>> {
    let mut out = Vec::new();
    let chars: Vec<(usize, char)> = src.char_indices().collect();
    let at = |i: usize| chars.get(i).map(|&(_, c)| c);
    let pos = |i: usize| chars.get(i).map_or(src.len(), |&(p, _)| p);
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i].1;
        let start = pos(i);
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        // Comments, which nest.
        if c == '(' && at(i + 1) == Some(':') {
            let mut depth = 1;
            i += 2;
            while i < chars.len() && depth > 0 {
                if at(i) == Some('(') && at(i + 1) == Some(':') {
                    depth += 1;
                    i += 2;
                } else if at(i) == Some(':') && at(i + 1) == Some(')') {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            if depth > 0 {
                return None;
            }
            continue;
        }
        let (kind, len) = match c {
            '"' | '\'' => {
                // Doubled quotes escape themselves.
                let mut j = i + 1;
                loop {
                    match at(j) {
                        None => return None,
                        Some(q) if q == c && at(j + 1) == Some(c) => j += 2,
                        Some(q) if q == c => break,
                        Some(_) => j += 1,
                    }
                }
                (Kind::String, j + 1 - i)
            }
            '$' => {
                let mut j = i + 1;
                while at(j).is_some_and(|c| is_name_char(c) || c == ':') {
                    j += 1;
                }
                (Kind::Variable, j - i)
            }
            '/' if at(i + 1) == Some('/') => (Kind::DoubleSlash, 2),
            '/' => (Kind::Slash, 1),
            '[' => (Kind::LBracket, 1),
            ']' => (Kind::RBracket, 1),
            '(' => (Kind::LParen, 1),
            ')' => (Kind::RParen, 1),
            ',' => (Kind::Comma, 1),
            '@' => (Kind::At, 1),
            '*' => (Kind::Star, 1),
            ':' if at(i + 1) == Some(':') => (Kind::Axis, 2),
            '.' if at(i + 1) == Some('.') => (Kind::DotDot, 2),
            '.' if !at(i + 1).is_some_and(|c| c.is_ascii_digit()) => (Kind::Dot, 1),
            '!' | '<' | '>' | ':' if at(i + 1) == Some('=') => (Kind::Op, 2),
            '<' if at(i + 1) == Some('<') => (Kind::Op, 2),
            '>' if at(i + 1) == Some('>') => (Kind::Op, 2),
            '=' | '<' | '>' | '|' | '+' | '-' | '?' | ':' => (Kind::Op, 1),
            c if c.is_ascii_digit() || c == '.' => {
                let mut j = i;
                while at(j).is_some_and(|c| c.is_ascii_digit() || c == '.') {
                    j += 1;
                }
                if matches!(at(j), Some('e' | 'E')) {
                    j += 1;
                    if matches!(at(j), Some('+' | '-')) {
                        j += 1;
                    }
                    while at(j).is_some_and(|c| c.is_ascii_digit()) {
                        j += 1;
                    }
                }
                (Kind::Number, j - i)
            }
            c if is_name_start(c) => {
                let mut j = i + 1;
                while at(j).is_some_and(is_name_char) {
                    j += 1;
                }
                // A prefixed name (`fo:block`, `xs:string`, `p:*`), but not
                // an axis (`child::`).
                if at(j) == Some(':') && at(j + 1) != Some(':') {
                    if at(j + 1) == Some('*') {
                        j += 2;
                    } else if at(j + 1).is_some_and(is_name_start) {
                        j += 2;
                        while at(j).is_some_and(is_name_char) {
                            j += 1;
                        }
                    }
                }
                (Kind::Name, j - i)
            }
            _ => return None,
        };
        let end = pos(i + len);
        out.push(Token {
            kind,
            span: start..end,
        });
        i += len;
    }
    Some(out)
}

/// For each opening bracket or parenthesis, the index of its partner.
pub fn partners(tokens: &[Token]) -> Option<Vec<Option<usize>>> {
    let mut out = vec![None; tokens.len()];
    let mut stack: Vec<usize> = Vec::new();
    for (i, t) in tokens.iter().enumerate() {
        match t.kind {
            Kind::LBracket | Kind::LParen => stack.push(i),
            Kind::RBracket | Kind::RParen => {
                let open = stack.pop()?;
                let matching = matches!(
                    (tokens[open].kind, t.kind),
                    (Kind::LBracket, Kind::RBracket) | (Kind::LParen, Kind::RParen)
                );
                if !matching {
                    return None;
                }
                out[open] = Some(i);
                out[i] = Some(open);
            }
            _ => {}
        }
    }
    stack.is_empty().then_some(out)
}

/// Whether the token before position `i` ends an operand, so that what
/// follows is an operator (`a and b`, `$x * 2`) rather than a new operand
/// (`and`, `*` as a name test). This is XPath's own rule for telling them
/// apart.
pub fn after_operand(tokens: &[Token], src: &str, i: usize) -> bool {
    let Some(prev) = i.checked_sub(1).map(|p| &tokens[p]) else {
        return false;
    };
    match prev.kind {
        Kind::Name => {
            // A keyword that takes an operand after it does not end one.
            !matches!(
                prev.text(src),
                "and"
                    | "or"
                    | "div"
                    | "mod"
                    | "idiv"
                    | "return"
                    | "then"
                    | "else"
                    | "in"
                    | "satisfies"
                    | "to"
                    | "eq"
                    | "ne"
                    | "lt"
                    | "le"
                    | "gt"
                    | "ge"
                    | "is"
                    | "union"
                    | "intersect"
                    | "except"
                    | "instance"
                    | "of"
                    | "treat"
                    | "as"
                    | "castable"
                    | "cast"
            ) || tokens.get(i).is_some_and(|t| t.kind == Kind::LParen)
        }
        Kind::Variable
        | Kind::String
        | Kind::Number
        | Kind::RBracket
        | Kind::RParen
        | Kind::Dot
        | Kind::DotDot
        | Kind::Star => true,
        _ => false,
    }
}

/// Node kind tests, which look like function calls but are steps.
pub fn is_kind_test(name: &str) -> bool {
    matches!(
        name,
        "node"
            | "text"
            | "comment"
            | "processing-instruction"
            | "element"
            | "attribute"
            | "document-node"
            | "schema-element"
            | "schema-attribute"
    )
}

/// If `tokens[from..]` starts with a simple path expression, the index just
/// past it. A simple path is made only of steps: a leading `$var`, `/` or
/// `//`, names, `*`, `@`, axes, `.`, `..`, kind tests and predicates in
/// brackets (whose contents can be anything). Its value is a sequence of
/// nodes.
pub fn simple_path_end(
    tokens: &[Token],
    src: &str,
    from: usize,
    pairs: &[Option<usize>],
) -> Option<usize> {
    let mut i = from;
    let mut steps = 0;
    // Whether the next token must start a step (after a slash or at the start).
    let mut want_step = true;
    while i < tokens.len() {
        let t = &tokens[i];
        match t.kind {
            Kind::Variable if i == from => {
                steps += 1;
                want_step = false;
                i += 1;
            }
            Kind::Slash | Kind::DoubleSlash if !want_step || i == from => {
                want_step = true;
                i += 1;
            }
            Kind::At if want_step => i += 1,
            Kind::Name if want_step => {
                let next = tokens.get(i + 1).map(|t| t.kind);
                if next == Some(Kind::Axis) {
                    i += 2;
                    continue;
                }
                if next == Some(Kind::LParen) {
                    // Only kind tests are steps; other calls end the path.
                    if !is_kind_test(t.text(src)) {
                        return None;
                    }
                    i = pairs[i + 1]? + 1;
                } else {
                    i += 1;
                }
                steps += 1;
                want_step = false;
            }
            Kind::Star | Kind::Dot | Kind::DotDot if want_step => {
                steps += 1;
                want_step = false;
                i += 1;
            }
            Kind::LBracket if !want_step => i = pairs[i]? + 1,
            _ => break,
        }
    }
    (steps > 0 && !want_step).then_some(i)
}

/// Functions whose result is a string, whatever their arguments.
pub fn returns_string(name: &str) -> bool {
    let local = name.strip_prefix("fn:").unwrap_or(name);
    matches!(
        local,
        "string"
            | "concat"
            | "normalize-space"
            | "lower-case"
            | "upper-case"
            | "substring"
            | "substring-before"
            | "substring-after"
            | "translate"
            | "string-join"
            | "normalize-unicode"
            | "replace"
            | "codepoints-to-string"
            | "local-name"
            | "name"
    )
}

/// Functions that are safe to move out of a predicate: their value does not
/// depend on the context item, position or size.
fn context_free_function(name: &str, argc: usize) -> bool {
    let local = name.strip_prefix("fn:").unwrap_or(name);
    // With no arguments these read the context item.
    let reads_context_without_args = matches!(
        local,
        "string"
            | "normalize-space"
            | "local-name"
            | "name"
            | "number"
            | "string-length"
            | "data"
            | "base-uri"
            | "root"
            | "lang"
    );
    if argc == 0 && reads_context_without_args {
        return false;
    }
    !matches!(
        local,
        "position" | "last" | "current-group" | "current-grouping-key"
    ) && (returns_string(name)
        || matches!(
            local,
            "string-length"
                | "number"
                | "data"
                | "current"
                | "true"
                | "false"
                | "not"
                | "boolean"
                | "count"
                | "exists"
                | "empty"
        ))
}

/// Whether `tokens[range]` is an expression whose value does not depend on
/// the focus (the context item, position or size): literals, variables,
/// `current()` and calls of known functions on such arguments.
pub fn context_free(
    tokens: &[Token],
    src: &str,
    range: Range<usize>,
    pairs: &[Option<usize>],
) -> bool {
    let mut i = range.start;
    while i < range.end {
        let t = &tokens[i];
        match t.kind {
            Kind::String
            | Kind::Number
            | Kind::Variable
            | Kind::Comma
            | Kind::LParen
            | Kind::RParen => i += 1,
            Kind::Op
                if matches!(
                    t.text(src),
                    "+" | "-" | "=" | "!=" | "<" | ">" | "<=" | ">="
                ) =>
            {
                i += 1
            }
            Kind::Name => {
                if tokens.get(i + 1).map(|t| t.kind) != Some(Kind::LParen) {
                    // A keyword operator is fine; a name is a path step.
                    if after_operand(tokens, src, i)
                        && matches!(
                            t.text(src),
                            "and" | "or" | "eq" | "ne" | "lt" | "le" | "gt" | "ge"
                        )
                    {
                        i += 1;
                        continue;
                    }
                    return false;
                }
                let Some(close) = pairs[i + 1] else {
                    return false;
                };
                if close > range.end {
                    return false;
                }
                let argc = if close == i + 2 {
                    0
                } else {
                    1 + tokens[i + 2..close]
                        .iter()
                        .filter(|t| t.kind == Kind::Comma)
                        .count()
                };
                if !context_free_function(t.text(src), argc) {
                    return false;
                }
                i += 2;
            }
            _ => return false,
        }
    }
    true
}

/// The token text with whitespace removed, for comparing two expressions.
pub fn canonical(tokens: &[Token], src: &str, range: Range<usize>) -> String {
    tokens[range]
        .iter()
        .map(|t| t.text(src))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<(Kind, &str)> {
        tokenize(src)
            .unwrap()
            .into_iter()
            .map(|t| (t.kind, &src[t.span]))
            .collect()
    }

    #[test]
    fn tokens_and_spans() {
        let src = "$list/item[@key = lower-case($k)][1]/@path (: note :) | //fo:block";
        let k = kinds(src);
        assert_eq!(k[0], (Kind::Variable, "$list"));
        assert_eq!(k[1], (Kind::Slash, "/"));
        assert_eq!(k[2], (Kind::Name, "item"));
        assert_eq!(k[4], (Kind::At, "@"));
        assert_eq!(k[6], (Kind::Op, "="));
        assert_eq!(k[7], (Kind::Name, "lower-case"));
        assert!(k.contains(&(Kind::DoubleSlash, "//")));
        assert_eq!(k.last().unwrap(), &(Kind::Name, "fo:block"));
        // Strings with doubled quotes, numbers, axes, wildcards.
        let k = kinds("'it''s' 1.5e3 ancestor::x p:* . .. a!=b");
        assert_eq!(k[0], (Kind::String, "'it''s'"));
        assert_eq!(k[1], (Kind::Number, "1.5e3"));
        assert_eq!(k[2], (Kind::Name, "ancestor"));
        assert_eq!(k[3], (Kind::Axis, "::"));
        assert_eq!(k[5], (Kind::Name, "p:*"));
        assert_eq!(k[8].1, "a");
        assert_eq!(k[9], (Kind::Op, "!="));
        assert!(tokenize("'open").is_none());
    }

    fn path_end(src: &str) -> Option<usize> {
        let t = tokenize(src).unwrap();
        let p = partners(&t).unwrap();
        simple_path_end(&t, src, 0, &p).map(|i| t.get(i).map_or(src.len(), |t| t.span.start))
    }

    #[test]
    fn simple_paths() {
        assert_eq!(path_end("$a/item[@k = 'x'][1]"), Some(20));
        assert_eq!(path_end("$a/item[1]/@path = 'x'"), Some(17));
        assert_eq!(path_end("//fig[@id=$i]/title"), Some(19));
        assert_eq!(path_end("ancestor::section[1]/text()"), Some(27));
        assert_eq!(path_end("string($a)"), None);
        assert_eq!(path_end("$a/count(x)"), None);
    }

    #[test]
    fn context_free_arguments() {
        let check = |src: &str| {
            let t = tokenize(src).unwrap();
            let p = partners(&t).unwrap();
            context_free(&t, src, 0..t.len(), &p)
        };
        assert!(check("$name"));
        assert!(check("lower-case($key)"));
        assert!(check("concat($a, '-', $b)"));
        assert!(!check("current()/@id"));
        assert!(!check("@id"));
        assert!(!check("."));
        assert!(!check("position()"));
        assert!(!check("string()"));
        assert!(!check("title"));
    }

    #[test]
    fn operators_after_operands() {
        let src = "a and b * 2";
        let t = tokenize(src).unwrap();
        assert!(after_operand(&t, src, 1)); // "and" follows an operand
        assert!(!after_operand(&t, src, 2)); // "b" follows "and"
        assert!(after_operand(&t, src, 3)); // "*" is multiplication
    }
}
