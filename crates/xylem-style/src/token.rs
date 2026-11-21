//! A small CSS tokenizer (a pragmatic subset of CSS Syntax Level 3).
//!
//! Beyond the standard, `$` is accepted as an identifier start so that XMetaL
//! pseudo-element names such as `$DOCUMENT` tokenize as identifiers.

/// A lexical CSS token.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Token {
    Ident(String),
    /// An identifier immediately followed by `(`.
    Function(String),
    AtKeyword(String),
    Hash(String),
    Str(String),
    Url(String),
    /// A number with its original source text.
    Number(f32, String),
    Percentage(f32),
    Dimension(f32, String),
    Delim(char),
    /// One of the attribute match operators `~=`, `|=`, `^=`, `$=`, `*=`;
    /// the character is the operator prefix.
    MatchOp(char),
    Colon,
    Semicolon,
    Comma,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    LParen,
    RParen,
    Whitespace,
}

/// Tokens plus the 1-based source line each token starts on.
#[derive(Debug, Default)]
pub(crate) struct Tokens {
    pub tokens: Vec<Token>,
    pub lines: Vec<u32>,
}

pub(crate) fn tokenize(src: &str) -> Tokens {
    let mut lexer = Lexer {
        chars: src.chars().collect(),
        pos: 0,
        line: 1,
    };
    let mut out = Tokens::default();
    while let Some((token, line)) = lexer.next_token() {
        out.tokens.push(token);
        out.lines.push(line);
    }
    out
}

struct Lexer {
    chars: Vec<char>,
    pos: usize,
    line: u32,
}

fn is_name_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || !c.is_ascii()
}

fn is_name_char(c: char) -> bool {
    is_name_start(c) || c.is_ascii_digit() || c == '-'
}

fn is_ws(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c' | '\u{feff}')
}

impl Lexer {
    fn peek(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.chars.get(self.pos).copied()?;
        self.pos += 1;
        if c == '\n' {
            self.line += 1;
        }
        Some(c)
    }

    fn starts_with(&self, s: &str) -> bool {
        s.chars().enumerate().all(|(i, c)| self.peek(i) == Some(c))
    }

    fn valid_escape_at(&self, offset: usize) -> bool {
        self.peek(offset) == Some('\\') && !matches!(self.peek(offset + 1), None | Some('\n'))
    }

    /// True if an identifier begins at the given offset.
    fn ident_start_at(&self, offset: usize) -> bool {
        match self.peek(offset) {
            Some('-') => match self.peek(offset + 1) {
                Some(c) if is_name_start(c) || c == '-' => true,
                _ => self.valid_escape_at(offset + 1),
            },
            Some('$') => matches!(self.peek(offset + 1), Some(c) if is_name_char(c)),
            Some('\\') => self.valid_escape_at(offset),
            Some(c) => is_name_start(c),
            None => false,
        }
    }

    fn number_start_at(&self, offset: usize) -> bool {
        match self.peek(offset) {
            Some(c) if c.is_ascii_digit() => true,
            Some('.') => matches!(self.peek(offset + 1), Some(c) if c.is_ascii_digit()),
            Some('+' | '-') => match self.peek(offset + 1) {
                Some(c) if c.is_ascii_digit() => true,
                Some('.') => matches!(self.peek(offset + 2), Some(c) if c.is_ascii_digit()),
                _ => false,
            },
            _ => false,
        }
    }

    /// Consumes an escape sequence; the backslash has already been consumed.
    fn consume_escape(&mut self) -> char {
        let Some(first) = self.peek(0) else {
            return '\u{fffd}';
        };
        if !first.is_ascii_hexdigit() {
            self.bump();
            return first;
        }
        let mut value = 0u32;
        let mut digits = 0;
        while digits < 6 {
            match self.peek(0).and_then(|c| c.to_digit(16)) {
                Some(d) => {
                    value = value * 16 + d;
                    digits += 1;
                    self.bump();
                }
                None => break,
            }
        }
        // A single whitespace character (or CRLF) terminates the escape.
        if self.peek(0) == Some('\r') && self.peek(1) == Some('\n') {
            self.bump();
            self.bump();
        } else if self.peek(0).is_some_and(is_ws) {
            self.bump();
        }
        char::from_u32(value)
            .filter(|&c| c != '\0')
            .unwrap_or('\u{fffd}')
    }

    fn consume_name(&mut self) -> String {
        let mut name = String::new();
        if self.peek(0) == Some('$') {
            name.push('$');
            self.bump();
        }
        loop {
            match self.peek(0) {
                Some(c) if is_name_char(c) => {
                    name.push(c);
                    self.bump();
                }
                Some('\\') if self.valid_escape_at(0) => {
                    self.bump();
                    name.push(self.consume_escape());
                }
                _ => break,
            }
        }
        name
    }

    fn consume_string(&mut self, quote: char) -> String {
        let mut s = String::new();
        while let Some(c) = self.peek(0) {
            match c {
                c if c == quote => {
                    self.bump();
                    break;
                }
                // An unescaped newline ends a malformed string.
                '\n' => break,
                '\\' => {
                    self.bump();
                    match self.peek(0) {
                        None => break,
                        Some('\n') => {
                            self.bump();
                        }
                        Some(_) => s.push(self.consume_escape()),
                    }
                }
                c => {
                    s.push(c);
                    self.bump();
                }
            }
        }
        s
    }

    fn consume_digits(&mut self, text: &mut String) {
        while let Some(c) = self.peek(0).filter(char::is_ascii_digit) {
            text.push(c);
            self.bump();
        }
    }

    fn consume_number(&mut self) -> (f32, String) {
        let mut text = String::new();
        if let Some(sign @ ('+' | '-')) = self.peek(0) {
            text.push(sign);
            self.bump();
        }
        self.consume_digits(&mut text);
        if self.peek(0) == Some('.') && matches!(self.peek(1), Some(c) if c.is_ascii_digit()) {
            text.push('.');
            self.bump();
            self.consume_digits(&mut text);
        }
        if matches!(self.peek(0), Some('e' | 'E')) {
            let sign = matches!(self.peek(1), Some('+' | '-'));
            let digit_at = if sign { 2 } else { 1 };
            if matches!(self.peek(digit_at), Some(c) if c.is_ascii_digit()) {
                for _ in 0..digit_at {
                    text.extend(self.bump());
                }
                self.consume_digits(&mut text);
            }
        }
        (text.parse().unwrap_or(0.0), text)
    }

    /// Consumes the unquoted body of `url(...)` after the opening parenthesis.
    fn consume_url(&mut self) -> String {
        while self.peek(0).is_some_and(is_ws) {
            self.bump();
        }
        let mut url = String::new();
        while let Some(c) = self.peek(0) {
            match c {
                ')' => {
                    self.bump();
                    return url;
                }
                c if is_ws(c) => {
                    while self.peek(0).is_some_and(is_ws) {
                        self.bump();
                    }
                    if self.peek(0) == Some(')') {
                        self.bump();
                    }
                    return url;
                }
                '\\' if self.valid_escape_at(0) => {
                    self.bump();
                    url.push(self.consume_escape());
                }
                c => {
                    url.push(c);
                    self.bump();
                }
            }
        }
        url
    }

    fn skip_comment(&mut self) {
        self.bump();
        self.bump();
        while self.peek(0).is_some() && !self.starts_with("*/") {
            self.bump();
        }
        if self.peek(0).is_some() {
            self.bump();
            self.bump();
        }
    }

    fn next_token(&mut self) -> Option<(Token, u32)> {
        // Comments and legacy HTML comment delimiters are insignificant.
        loop {
            if self.starts_with("/*") {
                self.skip_comment();
            } else if self.starts_with("<!--") {
                self.pos += 4;
            } else if self.starts_with("-->") {
                self.pos += 3;
            } else {
                break;
            }
        }
        let line = self.line;
        let c = self.peek(0)?;
        if is_ws(c) {
            loop {
                while self.peek(0).is_some_and(is_ws) {
                    self.bump();
                }
                if self.starts_with("/*") {
                    self.skip_comment();
                } else {
                    break;
                }
            }
            return Some((Token::Whitespace, line));
        }
        let token = match c {
            '"' | '\'' => {
                self.bump();
                Token::Str(self.consume_string(c))
            }
            '#' if matches!(self.peek(1), Some(n) if is_name_char(n))
                || self.valid_escape_at(1) =>
            {
                self.bump();
                Token::Hash(self.consume_name())
            }
            '@' if self.ident_start_at(1) => {
                self.bump();
                Token::AtKeyword(self.consume_name())
            }
            '~' | '|' | '^' | '$' | '*' if self.peek(1) == Some('=') => {
                self.pos += 2;
                Token::MatchOp(c)
            }
            _ if self.number_start_at(0) => {
                let (value, text) = self.consume_number();
                if self.peek(0) == Some('%') {
                    self.bump();
                    Token::Percentage(value)
                } else if self.ident_start_at(0) {
                    Token::Dimension(value, self.consume_name().to_ascii_lowercase())
                } else {
                    Token::Number(value, text)
                }
            }
            _ if self.ident_start_at(0) => {
                let name = self.consume_name();
                if self.peek(0) == Some('(') {
                    self.bump();
                    if name.eq_ignore_ascii_case("url") && !self.url_has_quote() {
                        Token::Url(self.consume_url())
                    } else {
                        Token::Function(name)
                    }
                } else {
                    Token::Ident(name)
                }
            }
            ':' => self.single(Token::Colon),
            ';' => self.single(Token::Semicolon),
            ',' => self.single(Token::Comma),
            '{' => self.single(Token::LBrace),
            '}' => self.single(Token::RBrace),
            '[' => self.single(Token::LBracket),
            ']' => self.single(Token::RBracket),
            '(' => self.single(Token::LParen),
            ')' => self.single(Token::RParen),
            other => self.single(Token::Delim(other)),
        };
        Some((token, line))
    }

    fn single(&mut self, token: Token) -> Token {
        self.bump();
        token
    }

    /// After `url(`: true if the argument starts with a quote.
    fn url_has_quote(&self) -> bool {
        let mut offset = 0;
        while self.peek(offset).is_some_and(is_ws) {
            offset += 1;
        }
        matches!(self.peek(offset), Some('"' | '\''))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(src: &str) -> Vec<Token> {
        tokenize(src).tokens
    }

    #[test]
    fn idents_numbers_and_dimensions() {
        assert_eq!(
            toks("a 1.5em 50% -3px #fff"),
            vec![
                Token::Ident("a".into()),
                Token::Whitespace,
                Token::Dimension(1.5, "em".into()),
                Token::Whitespace,
                Token::Percentage(50.0),
                Token::Whitespace,
                Token::Dimension(-3.0, "px".into()),
                Token::Whitespace,
                Token::Hash("fff".into()),
            ]
        );
    }

    #[test]
    fn exponent_is_not_confused_with_em() {
        assert_eq!(toks("1em"), vec![Token::Dimension(1.0, "em".into())]);
        assert_eq!(toks("1e2"), vec![Token::Number(100.0, "1e2".into())]);
    }

    #[test]
    fn string_escapes() {
        assert_eq!(toks(r#""\A""#), vec![Token::Str("\n".into())]);
        assert_eq!(toks(r#""\201C x""#), vec![Token::Str("\u{201c}x".into())]);
        assert_eq!(toks(r#"'a\'b'"#), vec![Token::Str("a'b".into())]);
        assert_eq!(toks("\"a\\\nb\""), vec![Token::Str("ab".into())]);
    }

    #[test]
    fn dollar_names_and_match_operators() {
        assert_eq!(toks("$DOCUMENT"), vec![Token::Ident("$DOCUMENT".into())]);
        assert_eq!(
            toks("[a$=v]"),
            vec![
                Token::LBracket,
                Token::Ident("a".into()),
                Token::MatchOp('$'),
                Token::Ident("v".into()),
                Token::RBracket
            ]
        );
    }

    #[test]
    fn urls() {
        assert_eq!(toks("url(a/b.gif)"), vec![Token::Url("a/b.gif".into())]);
        assert_eq!(
            toks("url( 'x.css' )"),
            vec![
                Token::Function("url".into()),
                Token::Whitespace,
                Token::Str("x.css".into()),
                Token::Whitespace,
                Token::RParen
            ]
        );
    }

    #[test]
    fn comments_and_lines() {
        let t = tokenize("a /* x\ny */ b\nc");
        assert_eq!(
            t.tokens,
            vec![
                Token::Ident("a".into()),
                Token::Whitespace,
                Token::Ident("b".into()),
                Token::Whitespace,
                Token::Ident("c".into())
            ]
        );
        assert_eq!(t.lines, vec![1, 1, 2, 2, 3]);
    }

    #[test]
    fn at_keywords_and_bom() {
        assert_eq!(
            toks("\u{feff}@sq-plus-minus;"),
            vec![
                Token::Whitespace,
                Token::AtKeyword("sq-plus-minus".into()),
                Token::Semicolon
            ]
        );
    }
}
