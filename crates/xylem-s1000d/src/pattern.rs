//! A small backtracking matcher for the regular-expression subset used by BREX
//! `valueForm="pattern"` rules (XML Schema style, always anchored to the whole value).
//!
//! Supported: literals, `.`, groups, `|`, character classes with ranges and
//! negation, the shorthands `\d \D \w \W \s \S`, and the quantifiers
//! `* + ? {n} {n,} {n,m}`.

#[derive(Debug, Clone)]
pub struct Pattern {
    alt: Alt,
}

#[derive(Debug, Clone)]
struct Alt(Vec<Vec<Piece>>);

#[derive(Debug, Clone)]
struct Piece {
    atom: Atom,
    min: usize,
    max: Option<usize>,
}

#[derive(Debug, Clone)]
enum Atom {
    Char(char),
    Any,
    Class(Class),
    Group(Alt),
}

#[derive(Debug, Clone)]
struct Class {
    negated: bool,
    items: Vec<ClassItem>,
}

#[derive(Debug, Clone)]
enum ClassItem {
    Range(char, char),
    Shorthand(char),
}

fn shorthand_matches(kind: char, c: char) -> bool {
    match kind {
        'd' => c.is_ascii_digit(),
        'D' => !c.is_ascii_digit(),
        'w' => c.is_alphanumeric() || c == '_',
        'W' => !(c.is_alphanumeric() || c == '_'),
        's' => c.is_whitespace(),
        'S' => !c.is_whitespace(),
        _ => false,
    }
}

impl Class {
    fn matches(&self, c: char) -> bool {
        let hit = self.items.iter().any(|item| match *item {
            ClassItem::Range(lo, hi) => lo <= c && c <= hi,
            ClassItem::Shorthand(kind) => shorthand_matches(kind, c),
        });
        hit != self.negated
    }
}

impl Pattern {
    pub fn new(source: &str) -> Result<Pattern, String> {
        let mut parser = Parser { chars: source.chars().collect(), pos: 0 };
        let alt = parser.alternation()?;
        if parser.pos < parser.chars.len() {
            return Err(format!("unmatched ')' at position {}", parser.pos));
        }
        Ok(Pattern { alt })
    }

    /// True if the whole of `text` matches the pattern.
    pub fn is_match(&self, text: &str) -> bool {
        let chars: Vec<char> = text.chars().collect();
        match_alt(&self.alt, &chars, 0, &mut |end| end == chars.len())
    }
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn alternation(&mut self) -> Result<Alt, String> {
        let mut branches = vec![self.sequence()?];
        while self.peek() == Some('|') {
            self.pos += 1;
            branches.push(self.sequence()?);
        }
        Ok(Alt(branches))
    }

    fn sequence(&mut self) -> Result<Vec<Piece>, String> {
        let mut pieces = Vec::new();
        while let Some(c) = self.peek() {
            if c == '|' || c == ')' {
                break;
            }
            self.pos += 1;
            let atom = match c {
                '^' | '$' => continue,
                '.' => Atom::Any,
                '(' => {
                    let inner = self.alternation()?;
                    if self.peek() != Some(')') {
                        return Err("missing ')'".to_string());
                    }
                    self.pos += 1;
                    Atom::Group(inner)
                }
                '[' => Atom::Class(self.class()?),
                '\\' => self.escape()?,
                '*' | '+' | '?' => return Err(format!("nothing to repeat before '{c}'")),
                other => Atom::Char(other),
            };
            let (min, max) = self.quantifier()?;
            pieces.push(Piece { atom, min, max });
        }
        Ok(pieces)
    }

    fn escape(&mut self) -> Result<Atom, String> {
        let c = self.peek().ok_or("dangling '\\'")?;
        self.pos += 1;
        Ok(match c {
            'd' | 'D' | 'w' | 'W' | 's' | 'S' => {
                Atom::Class(Class { negated: false, items: vec![ClassItem::Shorthand(c)] })
            }
            'n' => Atom::Char('\n'),
            't' => Atom::Char('\t'),
            'r' => Atom::Char('\r'),
            other => Atom::Char(other),
        })
    }

    fn class(&mut self) -> Result<Class, String> {
        let negated = self.peek() == Some('^');
        if negated {
            self.pos += 1;
        }
        let mut items = Vec::new();
        loop {
            let c = self.peek().ok_or("missing ']'")?;
            self.pos += 1;
            if c == ']' && !items.is_empty() {
                break;
            }
            let lo = if c == '\\' {
                let e = self.peek().ok_or("dangling '\\'")?;
                self.pos += 1;
                if matches!(e, 'd' | 'D' | 'w' | 'W' | 's' | 'S') {
                    items.push(ClassItem::Shorthand(e));
                    continue;
                }
                match e {
                    'n' => '\n',
                    't' => '\t',
                    'r' => '\r',
                    other => other,
                }
            } else {
                c
            };
            if self.peek() == Some('-') && self.chars.get(self.pos + 1).is_some_and(|&n| n != ']') {
                self.pos += 1;
                let mut hi = self.peek().ok_or("missing ']'")?;
                self.pos += 1;
                if hi == '\\' {
                    hi = self.peek().ok_or("dangling '\\'")?;
                    self.pos += 1;
                }
                if hi < lo {
                    return Err(format!("invalid range {lo}-{hi}"));
                }
                items.push(ClassItem::Range(lo, hi));
            } else {
                items.push(ClassItem::Range(lo, lo));
            }
        }
        Ok(Class { negated, items })
    }

    fn quantifier(&mut self) -> Result<(usize, Option<usize>), String> {
        match self.peek() {
            Some('*') => {
                self.pos += 1;
                Ok((0, None))
            }
            Some('+') => {
                self.pos += 1;
                Ok((1, None))
            }
            Some('?') => {
                self.pos += 1;
                Ok((0, Some(1)))
            }
            Some('{') => {
                let close = self.chars[self.pos..].iter().position(|&c| c == '}').ok_or("missing '}'")?;
                let body: String = self.chars[self.pos + 1..self.pos + close].iter().collect();
                self.pos += close + 1;
                let bad = || format!("invalid quantifier {{{body}}}");
                let (min, max) = match body.split_once(',') {
                    None => {
                        let n = body.trim().parse().map_err(|_| bad())?;
                        (n, Some(n))
                    }
                    Some((lo, hi)) if hi.trim().is_empty() => (lo.trim().parse().map_err(|_| bad())?, None),
                    Some((lo, hi)) => {
                        (lo.trim().parse().map_err(|_| bad())?, Some(hi.trim().parse().map_err(|_| bad())?))
                    }
                };
                if max.is_some_and(|m| m < min) {
                    return Err(bad());
                }
                Ok((min, max))
            }
            _ => Ok((1, Some(1))),
        }
    }
}

type Continuation<'a> = &'a mut dyn FnMut(usize) -> bool;

fn match_alt(alt: &Alt, s: &[char], pos: usize, k: Continuation<'_>) -> bool {
    alt.0.iter().any(|seq| match_seq(seq, s, pos, &mut *k))
}

fn match_seq(seq: &[Piece], s: &[char], pos: usize, k: Continuation<'_>) -> bool {
    match seq.split_first() {
        None => k(pos),
        Some((piece, rest)) => match_piece(piece, 0, s, pos, &mut |p| match_seq(rest, s, p, &mut *k)),
    }
}

fn match_piece(piece: &Piece, count: usize, s: &[char], pos: usize, k: Continuation<'_>) -> bool {
    let may_repeat = piece.max.is_none_or(|m| count < m);
    if may_repeat {
        let advanced = match_atom(&piece.atom, s, pos, &mut |p| {
            // An iteration that consumes nothing cannot make progress.
            if p == pos && count >= piece.min {
                return false;
            }
            match_piece(piece, count + 1, s, p, &mut *k)
        });
        if advanced {
            return true;
        }
    }
    count >= piece.min && k(pos)
}

fn match_atom(atom: &Atom, s: &[char], pos: usize, k: Continuation<'_>) -> bool {
    match atom {
        Atom::Char(c) => s.get(pos) == Some(c) && k(pos + 1),
        Atom::Any => s.get(pos).is_some_and(|&c| c != '\n') && k(pos + 1),
        Atom::Class(class) => s.get(pos).is_some_and(|&c| class.matches(c)) && k(pos + 1),
        Atom::Group(alt) => match_alt(alt, s, pos, k),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(pattern: &str, text: &str) -> bool {
        Pattern::new(pattern).unwrap_or_else(|e| panic!("{pattern}: {e}")).is_match(text)
    }

    #[test]
    fn literals_and_anchoring() {
        assert!(m("abc", "abc"));
        assert!(!m("abc", "abcd"));
        assert!(!m("abc", "xabc"));
        assert!(m("", ""));
        assert!(m("^abc$", "abc"));
    }

    #[test]
    fn alternation_and_groups() {
        assert!(m("cat|dog", "dog"));
        assert!(!m("cat|dog", "do"));
        assert!(m("(ab)+c", "ababc"));
        assert!(m("a(b|c)d", "acd"));
        assert!(m("(a|)b", "b"));
    }

    #[test]
    fn classes_and_shorthands() {
        assert!(m("[A-Z][0-9]+", "A123"));
        assert!(!m("[A-Z][0-9]+", "a123"));
        assert!(m("[^0-9]+", "abc"));
        assert!(!m("[^0-9]+", "ab1"));
        assert!(m(r"\d{3}-\d{2}", "012-34"));
        assert!(m(r"[\d.]+", "1.25"));
        assert!(m(r"\w+\s\w+", "ab cd"));
        assert!(m("[a-c-]+", "a-b"));
        assert!(m(".", "x"));
        assert!(!m(".", ""));
    }

    #[test]
    fn quantifiers() {
        assert!(m("a*", ""));
        assert!(m("a+", "aaa"));
        assert!(!m("a+", ""));
        assert!(m("ab?c", "ac"));
        assert!(m("a{2,3}", "aaa"));
        assert!(!m("a{2,3}", "aaaa"));
        assert!(m("a{2,}", "aaaaa"));
        assert!(m("(a*)*b", "aab"));
        assert!(m("a{0}b", "b"));
    }

    #[test]
    fn rejects_bad_patterns() {
        for bad in ["(ab", "ab)", "[a-", "*a", "a{2,1}", "a{x}", "[z-a]", "\\"] {
            assert!(Pattern::new(bad).is_err(), "{bad}");
        }
    }
}
