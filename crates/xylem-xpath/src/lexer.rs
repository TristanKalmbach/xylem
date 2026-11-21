//! Tokenizer implementing the disambiguation rules of XPath 1.0 section 3.7.

use crate::error::XPathError;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Token {
    Number(f64),
    Literal(String),
    Variable(String),
    /// A QName used as a name test.
    Name(String),
    /// `prefix:*`
    PrefixStar(String),
    /// `*` as a name test.
    Star,
    /// `*` as the multiplication operator.
    Multiply,
    NodeType(String),
    FunctionName(String),
    AxisName(String),
    And,
    Or,
    Mod,
    Div,
    Slash,
    DoubleSlash,
    Pipe,
    Plus,
    Minus,
    Eq,
    Neq,
    Lt,
    Le,
    Gt,
    Ge,
    Dot,
    DotDot,
    At,
    ColonColon,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
}

impl Token {
    fn is_operator(&self) -> bool {
        matches!(
            self,
            Token::And
                | Token::Or
                | Token::Mod
                | Token::Div
                | Token::Multiply
                | Token::Slash
                | Token::DoubleSlash
                | Token::Pipe
                | Token::Plus
                | Token::Minus
                | Token::Eq
                | Token::Neq
                | Token::Lt
                | Token::Le
                | Token::Gt
                | Token::Ge
        )
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Spanned {
    pub token: Token,
    pub pos: usize,
}

fn is_name_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '.') || c as u32 == 0xB7
}

pub(crate) fn tokenize(src: &str) -> Result<Vec<Spanned>, XPathError> {
    let chars: Vec<char> = src.chars().collect();
    let mut out: Vec<Spanned> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        // After one of these tokens (or at the start) a name is a name and `*` is a wildcard.
        let operator_expected = match out.last() {
            None => false,
            Some(s) => !(matches!(
                s.token,
                Token::At | Token::ColonColon | Token::LParen | Token::LBracket | Token::Comma
            ) || s.token.is_operator()),
        };
        let next_is = |k: usize, want: char| chars.get(k) == Some(&want);
        let token = match c {
            '(' => {
                i += 1;
                Token::LParen
            }
            ')' => {
                i += 1;
                Token::RParen
            }
            '[' => {
                i += 1;
                Token::LBracket
            }
            ']' => {
                i += 1;
                Token::RBracket
            }
            ',' => {
                i += 1;
                Token::Comma
            }
            '@' => {
                i += 1;
                Token::At
            }
            '|' => {
                i += 1;
                Token::Pipe
            }
            '+' => {
                i += 1;
                Token::Plus
            }
            '-' => {
                i += 1;
                Token::Minus
            }
            '=' => {
                i += 1;
                Token::Eq
            }
            '!' => {
                if next_is(i + 1, '=') {
                    i += 2;
                    Token::Neq
                } else {
                    return Err(XPathError::syntax(i, "expected '=' after '!'"));
                }
            }
            '<' => {
                if next_is(i + 1, '=') {
                    i += 2;
                    Token::Le
                } else {
                    i += 1;
                    Token::Lt
                }
            }
            '>' => {
                if next_is(i + 1, '=') {
                    i += 2;
                    Token::Ge
                } else {
                    i += 1;
                    Token::Gt
                }
            }
            '/' => {
                if next_is(i + 1, '/') {
                    i += 2;
                    Token::DoubleSlash
                } else {
                    i += 1;
                    Token::Slash
                }
            }
            ':' => {
                if next_is(i + 1, ':') {
                    i += 2;
                    Token::ColonColon
                } else {
                    return Err(XPathError::syntax(i, "unexpected ':'"));
                }
            }
            '*' => {
                i += 1;
                if operator_expected { Token::Multiply } else { Token::Star }
            }
            '"' | '\'' => {
                let quote = c;
                i += 1;
                let begin = i;
                while i < chars.len() && chars[i] != quote {
                    i += 1;
                }
                if i >= chars.len() {
                    return Err(XPathError::syntax(start, "unterminated string literal"));
                }
                let s: String = chars[begin..i].iter().collect();
                i += 1;
                Token::Literal(s)
            }
            '$' => {
                i += 1;
                let (name, next) = read_qname(&chars, i)
                    .ok_or_else(|| XPathError::syntax(i, "expected variable name after '$'"))?;
                i = next;
                Token::Variable(name)
            }
            '.' => {
                if next_is(i + 1, '.') {
                    i += 2;
                    Token::DotDot
                } else if chars.get(i + 1).is_some_and(|d| d.is_ascii_digit()) {
                    let (n, next) = read_number(&chars, i);
                    i = next;
                    Token::Number(n)
                } else {
                    i += 1;
                    Token::Dot
                }
            }
            d if d.is_ascii_digit() => {
                let (n, next) = read_number(&chars, i);
                i = next;
                Token::Number(n)
            }
            n if is_name_start(n) => {
                let (name, next) = lex_name(&chars, i)?;
                i = next;
                if operator_expected {
                    match name.as_str() {
                        "and" => Token::And,
                        "or" => Token::Or,
                        "mod" => Token::Mod,
                        "div" => Token::Div,
                        _ => {
                            return Err(XPathError::syntax(
                                start,
                                format!("expected an operator, found '{name}'"),
                            ));
                        }
                    }
                } else if let Some(prefix) = name.strip_suffix(":*") {
                    Token::PrefixStar(prefix.to_string())
                } else {
                    let mut k = i;
                    while k < chars.len() && chars[k].is_whitespace() {
                        k += 1;
                    }
                    if next_is(k, '(') {
                        if matches!(name.as_str(), "comment" | "text" | "processing-instruction" | "node") {
                            Token::NodeType(name)
                        } else {
                            Token::FunctionName(name)
                        }
                    } else if next_is(k, ':') && next_is(k + 1, ':') {
                        Token::AxisName(name)
                    } else {
                        Token::Name(name)
                    }
                }
            }
            other => {
                return Err(XPathError::syntax(i, format!("unexpected character '{other}'")));
            }
        };
        out.push(Spanned { token, pos: start });
    }
    Ok(out)
}

fn read_number(chars: &[char], mut i: usize) -> (f64, usize) {
    let begin = i;
    while i < chars.len() && chars[i].is_ascii_digit() {
        i += 1;
    }
    if i < chars.len() && chars[i] == '.' {
        i += 1;
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
    }
    let text: String = chars[begin..i].iter().collect();
    (text.parse::<f64>().unwrap_or(f64::NAN), i)
}

/// Reads an NCName starting at `i`.
fn read_ncname(chars: &[char], mut i: usize) -> Option<(String, usize)> {
    if i >= chars.len() || !is_name_start(chars[i]) {
        return None;
    }
    let begin = i;
    while i < chars.len() && is_name_char(chars[i]) {
        i += 1;
    }
    Some((chars[begin..i].iter().collect(), i))
}

/// Reads `NCName` or `NCName:NCName`.
fn read_qname(chars: &[char], i: usize) -> Option<(String, usize)> {
    let (mut name, mut next) = read_ncname(chars, i)?;
    if chars.get(next) == Some(&':')
        && chars.get(next + 1) != Some(&':')
        && let Some((local, after)) = read_ncname(chars, next + 1)
    {
        name.push(':');
        name.push_str(&local);
        next = after;
    }
    Some((name, next))
}

/// Reads a QName or `prefix:*`; the wildcard form is returned with a `:*` suffix.
fn lex_name(chars: &[char], i: usize) -> Result<(String, usize), XPathError> {
    let (first, next) = read_ncname(chars, i).ok_or_else(|| XPathError::syntax(i, "expected a name"))?;
    if chars.get(next) == Some(&':') && chars.get(next + 1) != Some(&':') {
        if chars.get(next + 1) == Some(&'*') {
            return Ok((format!("{first}:*"), next + 2));
        }
        if let Some((local, after)) = read_ncname(chars, next + 1) {
            return Ok((format!("{first}:{local}"), after));
        }
        return Err(XPathError::syntax(next, "expected a local name after ':'"));
    }
    Ok((first, next))
}
