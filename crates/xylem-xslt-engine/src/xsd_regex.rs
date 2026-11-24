//! XPath 2.0 / XML Schema regular expressions.
//!
//! XPath patterns (`fn:matches`, `fn:replace`, `fn:tokenize`) use the XML
//! Schema regex syntax plus anchors, reluctant quantifiers and
//! back-references. This module parses that syntax, rejects invalid
//! patterns with `FORX0002`, and translates valid ones into the `regex`
//! crate's syntax. Patterns with back-references, which the `regex` crate
//! cannot express, are compiled with `fancy-regex` (a backtracking engine).

use std::borrow::Cow;

/// A compiled XPath regular expression.
pub enum XRegex {
    /// A pattern the `regex` crate can run directly.
    Plain(regex::Regex),
    /// A pattern with back-references, run by the backtracking engine.
    Backtracking(fancy_regex::Regex),
}

const ERR_PATTERN: &str = "FORX0002";
const ERR_FLAGS: &str = "FORX0001";

fn invalid(pattern: &str, why: &str) -> String {
    format!("{ERR_PATTERN}: invalid regular expression '{pattern}': {why}")
}

impl XRegex {
    /// Compiles `pattern` with the XPath `flags` (`s`, `m`, `i`, `x`, `q`),
    /// accepting XPath 3.0 syntax.
    #[cfg(test)]
    pub fn new(pattern: &str, flags: &str) -> Result<Self, String> {
        Self::with_version(pattern, flags, true)
    }

    /// Compiles `pattern`; without `xpath30`, the XPath 3.0 additions
    /// (non-capturing groups, the `q` flag) are errors.
    pub fn with_version(pattern: &str, flags: &str, xpath30: bool) -> Result<Self, String> {
        let mut f = Flags::default();
        for c in flags.chars() {
            match c {
                's' => f.dot_all = true,
                'm' => f.multi_line = true,
                'i' => f.case_insensitive = true,
                'x' => f.extended = true,
                'q' if xpath30 => f.literal = true,
                _ => return Err(format!("{ERR_FLAGS}: unknown regex flag '{c}'")),
            }
        }
        let (body, backrefs) = if f.literal {
            (regex::escape(pattern), false)
        } else {
            let mut t = Translator::new(pattern, &f);
            t.xpath30 = xpath30;
            t.translate().map_err(|why| invalid(pattern, &why))?;
            (t.out, t.uses_backrefs)
        };
        let mut prefix = String::new();
        if f.multi_line {
            prefix.push('m');
        }
        if f.case_insensitive {
            prefix.push('i');
        }
        let full = if prefix.is_empty() {
            body
        } else {
            format!("(?{prefix}){body}")
        };
        if backrefs {
            fancy_regex::Regex::new(&full)
                .map(XRegex::Backtracking)
                .map_err(|e| invalid(pattern, &e.to_string()))
        } else {
            regex::RegexBuilder::new(&full)
                .size_limit(64 << 20)
                .build()
                .map(XRegex::Plain)
                .map_err(|e| invalid(pattern, &e.to_string()))
        }
    }

    /// The number of capturing groups.
    pub fn groups(&self) -> usize {
        match self {
            XRegex::Plain(r) => r.captures_len() - 1,
            XRegex::Backtracking(r) => r.captures_len() - 1,
        }
    }

    /// Whether the pattern matches somewhere in `s`.
    pub fn is_match(&self, s: &str) -> Result<bool, String> {
        match self {
            XRegex::Plain(r) => Ok(r.is_match(s)),
            XRegex::Backtracking(r) => r.is_match(s).map_err(|e| e.to_string()),
        }
    }

    /// Replaces every match; `rep` uses the `regex` crate's `${n}` syntax.
    pub fn replace_all<'t>(&self, s: &'t str, rep: &str) -> Cow<'t, str> {
        match self {
            XRegex::Plain(r) => r.replace_all(s, rep),
            XRegex::Backtracking(r) => r.replace_all(s, rep),
        }
    }

    /// Every match as (start, end, groups); group 0 is the whole match and
    /// a group that did not participate is "".
    pub fn captures(&self, s: &str) -> Result<Vec<(usize, usize, Vec<String>)>, String> {
        let mut out = Vec::new();
        match self {
            XRegex::Plain(r) => {
                for c in r.captures_iter(s) {
                    let m = c.get(0).map_or((0, 0), |m| (m.start(), m.end()));
                    let g = c.iter().map(|x| x.map_or("", |x| x.as_str()).to_string());
                    out.push((m.0, m.1, g.collect()));
                }
            }
            XRegex::Backtracking(r) => {
                for c in r.captures_iter(s) {
                    let c = c.map_err(|e| e.to_string())?;
                    let m = c.get(0).map_or((0, 0), |m| (m.start(), m.end()));
                    let g = c.iter().map(|x| x.map_or("", |x| x.as_str()).to_string());
                    out.push((m.0, m.1, g.collect()));
                }
            }
        }
        Ok(out)
    }

    /// Splits `s` at each match.
    pub fn split(&self, s: &str) -> Result<Vec<String>, String> {
        match self {
            XRegex::Plain(r) => Ok(r.split(s).map(str::to_string).collect()),
            XRegex::Backtracking(r) => r
                .split(s)
                .map(|p| p.map(str::to_string).map_err(|e| e.to_string()))
                .collect(),
        }
    }
}

#[derive(Default)]
struct Flags {
    dot_all: bool,
    multi_line: bool,
    case_insensitive: bool,
    extended: bool,
    literal: bool,
}

/// Recursive-descent translator over the pattern's characters.
struct Translator {
    src: Vec<char>,
    pos: usize,
    out: String,
    dot_all: bool,
    /// Number of capturing groups opened so far.
    groups: usize,
    /// Capturing groups whose closing parenthesis has been seen.
    closed: Vec<usize>,
    uses_backrefs: bool,
    /// Under the `i` flag, `\p{..}` still tests the exact character.
    case_insensitive: bool,
    /// Accept XPath 3.0 additions such as `(?:...)`.
    xpath30: bool,
}

type T<V> = Result<V, String>;

impl Translator {
    fn new(pattern: &str, f: &Flags) -> Self {
        let mut src: Vec<char> = pattern.chars().collect();
        if f.extended {
            // XPath 3.0: whitespace is removed except inside character classes.
            let mut kept = Vec::with_capacity(src.len());
            let mut depth = 0usize;
            let mut i = 0;
            while i < src.len() {
                let c = src[i];
                if c == '\\' && i + 1 < src.len() && !src[i + 1].is_whitespace() {
                    kept.push(c);
                    kept.push(src[i + 1]);
                    i += 2;
                    continue;
                }
                match c {
                    '[' => depth += 1,
                    ']' => depth = depth.saturating_sub(1),
                    _ => {}
                }
                if depth > 0 || !matches!(c, ' ' | '\t' | '\n' | '\r') {
                    kept.push(c);
                }
                i += 1;
            }
            src = kept;
        }
        Translator {
            src,
            pos: 0,
            out: String::new(),
            dot_all: f.dot_all,
            groups: 0,
            closed: Vec::new(),
            uses_backrefs: false,
            case_insensitive: f.case_insensitive,
            xpath30: true,
        }
    }

    fn peek(&self) -> Option<char> {
        self.src.get(self.pos).copied()
    }

    fn peek_at(&self, k: usize) -> Option<char> {
        self.src.get(self.pos + k).copied()
    }

    fn translate(&mut self) -> T<()> {
        self.branches()?;
        match self.peek() {
            None => Ok(()),
            Some(')') => Err("unmatched ')'".into()),
            Some(c) => Err(format!("unexpected '{c}'")),
        }
    }

    /// regExp ::= branch ( '|' branch )*
    fn branches(&mut self) -> T<()> {
        self.branch()?;
        while self.peek() == Some('|') {
            self.pos += 1;
            self.out.push('|');
            self.branch()?;
        }
        Ok(())
    }

    /// branch ::= piece*
    fn branch(&mut self) -> T<()> {
        while let Some(c) = self.peek() {
            if c == '|' || c == ')' {
                break;
            }
            self.piece()?;
        }
        Ok(())
    }

    /// piece ::= atom quantifier?
    fn piece(&mut self) -> T<()> {
        let quantifiable = self.atom()?;
        let Some(c) = self.peek() else { return Ok(()) };
        if !matches!(c, '?' | '*' | '+' | '{') {
            return Ok(());
        }
        if !quantifiable {
            return Err(format!("quantifier '{c}' has nothing to repeat"));
        }
        self.quantifier()?;
        if self.peek() == Some('?') {
            self.pos += 1;
            self.out.push('?');
        }
        if matches!(self.peek(), Some('?' | '*' | '+' | '{')) {
            return Err("a quantifier cannot follow another quantifier".into());
        }
        Ok(())
    }

    fn quantifier(&mut self) -> T<()> {
        let c = self.src[self.pos];
        self.pos += 1;
        if c != '{' {
            self.out.push(c);
            return Ok(());
        }
        let min = self.digits().ok_or("quantifier '{' needs a minimum")?;
        let mut q = format!("{{{min}");
        match self.peek() {
            Some('}') => {}
            Some(',') => {
                self.pos += 1;
                q.push(',');
                if let Some(max) = self.digits() {
                    if parse_bound(&max)? < parse_bound(&min)? {
                        return Err(format!("quantifier {{{min},{max}}} has max < min"));
                    }
                    q.push_str(&max);
                }
            }
            _ => return Err("malformed quantifier".into()),
        }
        if self.peek() != Some('}') {
            return Err("unterminated quantifier".into());
        }
        self.pos += 1;
        q.push('}');
        self.out.push_str(&q);
        Ok(())
    }

    fn digits(&mut self) -> Option<String> {
        let st = self.pos;
        while matches!(self.peek(), Some('0'..='9')) {
            self.pos += 1;
        }
        (self.pos > st).then(|| self.src[st..self.pos].iter().collect())
    }

    /// Emits one atom. Returns whether a quantifier may follow it.
    fn atom(&mut self) -> T<bool> {
        let c = self.src[self.pos];
        self.pos += 1;
        match c {
            '(' => {
                let capturing = !(self.peek() == Some('?') && self.peek_at(1) == Some(':'));
                if capturing {
                    if self.peek() == Some('?') {
                        return Err("unsupported group construct '(?'".into());
                    }
                    self.groups += 1;
                    self.out.push('(');
                } else if !self.xpath30 {
                    return Err("non-capturing groups '(?:' need XPath 3.0".into());
                } else {
                    self.pos += 2;
                    self.out.push_str("(?:");
                }
                let n = self.groups;
                self.branches()?;
                if self.peek() != Some(')') {
                    return Err("missing ')'".into());
                }
                self.pos += 1;
                self.out.push(')');
                if capturing {
                    self.closed.push(n);
                }
                Ok(true)
            }
            '[' => {
                let class = self.class()?;
                self.out.push_str(&class);
                Ok(true)
            }
            '.' => {
                self.out
                    .push_str(if self.dot_all { "(?s:.)" } else { "[^\\n\\r]" });
                Ok(true)
            }
            '^' => {
                self.out.push('^');
                Ok(false)
            }
            '$' => {
                self.out.push('$');
                Ok(false)
            }
            '\\' => {
                if let Some(d @ '1'..='9') = self.peek() {
                    self.backref(d)?;
                    return Ok(true);
                }
                let e = self.escape(false)?;
                self.out.push_str(&e);
                Ok(true)
            }
            '?' | '*' | '+' | '{' => Err(format!("quantifier '{c}' has nothing to repeat")),
            '}' | ']' => Err(format!("unescaped '{c}'")),
            _ => {
                push_literal(&mut self.out, c);
                Ok(true)
            }
        }
    }

    /// `\N`: the longest digit run naming a group that is already closed.
    fn backref(&mut self, first: char) -> T<()> {
        self.pos += 1;
        let mut n = first.to_digit(10).unwrap_or(0) as usize;
        while let Some(d) = self.peek().and_then(|c| c.to_digit(10)) {
            let longer = n * 10 + d as usize;
            if longer > self.groups {
                break;
            }
            n = longer;
            self.pos += 1;
        }
        if !self.closed.contains(&n) {
            return Err(format!("back-reference \\{n} to a group that is not closed"));
        }
        self.uses_backrefs = true;
        // Wrap so that following digits are not read as part of the number.
        self.out.push_str(&format!("(?:\\{n})"));
        Ok(())
    }

    /// Parses an escape after `\` and returns it as a `regex` crate atom
    /// (a literal or a bracketed class). `in_class` permits nothing extra
    /// but is kept for error messages.
    fn escape(&mut self, in_class: bool) -> T<String> {
        let Some(c) = self.peek() else {
            return Err("pattern ends with '\\'".into());
        };
        self.pos += 1;
        let s: Cow<str> = match c {
            'n' => "\\n".into(),
            'r' => "\\r".into(),
            't' => "\\t".into(),
            '\\' | '|' | '.' | '?' | '*' | '+' | '(' | ')' | '{' | '}' | '-' | '[' | ']' | '^'
            | '$' => format!("\\{c}").into(),
            's' => "[\\t\\n\\r ]".into(),
            'S' => "[^\\t\\n\\r ]".into(),
            'd' => "\\p{Nd}".into(),
            'D' => "\\P{Nd}".into(),
            'w' => "[^\\p{P}\\p{Z}\\p{C}]".into(),
            'W' => "[\\p{P}\\p{Z}\\p{C}]".into(),
            'i' => format!("[{NAME_START}]").into(),
            'I' => format!("[^{NAME_START}]").into(),
            'c' => format!("[{NAME_START}{NAME_EXTRA}]").into(),
            'C' => format!("[^{NAME_START}{NAME_EXTRA}]").into(),
            'p' | 'P' if !in_class && self.case_insensitive => {
                format!("(?-i:{})", self.property(c == 'P')?).into()
            }
            'p' | 'P' => self.property(c == 'P')?.into(),
            _ => {
                let place = if in_class { " in a character class" } else { "" };
                return Err(format!("unknown escape '\\{c}'{place}"));
            }
        };
        Ok(s.into_owned())
    }

    /// `\p{..}` / `\P{..}`: a general category or an `Is` block name.
    fn property(&mut self, negated: bool) -> T<String> {
        if self.peek() != Some('{') {
            return Err("'\\p' needs '{'".into());
        }
        self.pos += 1;
        let st = self.pos;
        while self.peek().is_some_and(|c| c != '}') {
            self.pos += 1;
        }
        if self.peek().is_none() {
            return Err("unterminated '\\p{'".into());
        }
        let name: String = self.src[st..self.pos].iter().collect();
        self.pos += 1;
        if let Some(block) = name.strip_prefix("Is") {
            let ranges = block_ranges(block)
                .ok_or_else(|| format!("unknown block name '{name}'"))?;
            let mut set = String::new();
            for &(lo, hi) in ranges {
                // Surrogates cannot occur in a string; leave them out.
                let parts: &[(u32, u32)] = &[(lo, hi.min(0xD7FF)), (lo.max(0xE000), hi)];
                for &(a, b) in parts {
                    if a <= b {
                        set.push_str(&format!("\\x{{{a:X}}}-\\x{{{b:X}}}"));
                    }
                }
            }
            if set.is_empty() {
                return Ok(if negated { "(?s:.)".into() } else { EMPTY_CLASS.into() });
            }
            return Ok(format!("[{}{set}]", if negated { "^" } else { "" }));
        }
        if !CATEGORIES.contains(&name.as_str()) {
            return Err(format!("unknown character property '{name}'"));
        }
        Ok(format!("\\{}{{{name}}}", if negated { 'P' } else { 'p' }))
    }

    /// Parses a character class after `[` and returns the translated class.
    fn class(&mut self) -> T<String> {
        let negated = self.peek() == Some('^');
        if negated {
            self.pos += 1;
        }
        let mut items = String::new();
        let mut first = true;
        let mut subtraction = None;
        loop {
            let Some(c) = self.peek() else {
                return Err("unterminated character class".into());
            };
            match c {
                ']' if first => return Err("empty character class".into()),
                ']' => {
                    self.pos += 1;
                    break;
                }
                '-' if self.peek_at(1) == Some('[') && !first => {
                    self.pos += 2;
                    subtraction = Some(self.class()?);
                    if self.peek() != Some(']') {
                        return Err("a class subtraction must end the class".into());
                    }
                    self.pos += 1;
                    break;
                }
                _ => {}
            }
            first = false;
            let lo = self.class_atom()?;
            match lo {
                ClassAtom::Char(a) if self.peek() == Some('-') && self.peek_at(1) != Some(']')
                    && self.peek_at(1) != Some('[') =>
                {
                    self.pos += 1;
                    let ClassAtom::Char(b) = self.class_atom()? else {
                        return Err("a range end must be a single character".into());
                    };
                    if b < a {
                        return Err(format!("range {a}-{b} is out of order"));
                    }
                    push_class_char(&mut items, a);
                    items.push('-');
                    push_class_char(&mut items, b);
                }
                ClassAtom::Char(a) => push_class_char(&mut items, a),
                ClassAtom::Set(s) => items.push_str(&s),
            }
        }
        let base = format!("[{}{items}]", if negated { "^" } else { "" });
        Ok(match subtraction {
            Some(sub) => format!("[{base}--{sub}]"),
            None => base,
        })
    }

    fn class_atom(&mut self) -> T<ClassAtom> {
        let c = self.src[self.pos];
        self.pos += 1;
        match c {
            '\\' => {
                let Some(e) = self.peek() else {
                    return Err("pattern ends with '\\'".into());
                };
                let single = match e {
                    'n' => Some('\n'),
                    'r' => Some('\r'),
                    't' => Some('\t'),
                    '\\' | '|' | '.' | '?' | '*' | '+' | '(' | ')' | '{' | '}' | '-' | '['
                    | ']' | '^' | '$' => Some(e),
                    _ => None,
                };
                if let Some(ch) = single {
                    self.pos += 1;
                    return Ok(ClassAtom::Char(ch));
                }
                Ok(ClassAtom::Set(self.escape(true)?))
            }
            '[' => Err("'[' must be escaped inside a character class".into()),
            _ => Ok(ClassAtom::Char(c)),
        }
    }
}

enum ClassAtom {
    Char(char),
    /// A translated class or property (`[..]`, `\p{..}`).
    Set(String),
}

fn parse_bound(s: &str) -> T<u64> {
    s.parse().map_err(|_| format!("quantifier bound {s} is too large"))
}

/// Whether `c` is an XML 1.0 `Char`.
pub fn is_xml_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}')
        || c >= '\u{10000}'
}

/// A class that matches nothing.
const EMPTY_CLASS: &str = "[a&&b]";

/// XML 1.0 (fifth edition) NameStartChar, as `regex` class items.
const NAME_START: &str = ":A-Z_a-z\\x{C0}-\\x{D6}\\x{D8}-\\x{F6}\\x{F8}-\\x{2FF}\\x{370}-\\x{37D}\
\\x{37F}-\\x{1FFF}\\x{200C}-\\x{200D}\\x{2070}-\\x{218F}\\x{2C00}-\\x{2FEF}\\x{3001}-\\x{D7FF}\
\\x{F900}-\\x{FDCF}\\x{FDF0}-\\x{FFFD}\\x{10000}-\\x{EFFFF}";

/// Characters NameChar adds to NameStartChar.
const NAME_EXTRA: &str = "\\-.0-9\\x{B7}\\x{300}-\\x{36F}\\x{203F}-\\x{2040}";

/// The general categories XML Schema allows in `\p{..}`.
const CATEGORIES: &[&str] = &[
    "L", "Lu", "Ll", "Lt", "Lm", "Lo", "M", "Mn", "Mc", "Me", "N", "Nd", "Nl", "No", "P", "Pc",
    "Pd", "Ps", "Pe", "Pi", "Pf", "Po", "Z", "Zs", "Zl", "Zp", "S", "Sm", "Sc", "Sk", "So", "C",
    "Cc", "Cf", "Co", "Cn",
];

fn push_literal(out: &mut String, c: char) {
    if regex_syntax_meta(c) || c == ' ' {
        out.push('\\');
    }
    out.push(c);
}

/// Characters the `regex` crate treats as syntax (in or out of classes).
fn regex_syntax_meta(c: char) -> bool {
    matches!(
        c,
        '\\' | '.' | '+' | '*' | '?' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$'
            | '#' | '&' | '-' | '~'
    )
}

fn push_class_char(out: &mut String, c: char) {
    match c {
        '\n' => out.push_str("\\n"),
        '\r' => out.push_str("\\r"),
        '\t' => out.push_str("\\t"),
        _ => push_literal(out, c),
    }
}

/// The ranges of an XML Schema block name (`\p{IsBasicLatin}` and so on),
/// following the Unicode block list XML Schema 1.0 refers to.
fn block_ranges(name: &str) -> Option<&'static [(u32, u32)]> {
    let r: &'static [(u32, u32)] = match name {
        "BasicLatin" => &[(0x0000, 0x007F)],
        "Latin-1Supplement" => &[(0x0080, 0x00FF)],
        "LatinExtended-A" => &[(0x0100, 0x017F)],
        "LatinExtended-B" => &[(0x0180, 0x024F)],
        "IPAExtensions" => &[(0x0250, 0x02AF)],
        "SpacingModifierLetters" => &[(0x02B0, 0x02FF)],
        "CombiningDiacriticalMarks" => &[(0x0300, 0x036F)],
        "Greek" | "GreekandCoptic" => &[(0x0370, 0x03FF)],
        "Cyrillic" => &[(0x0400, 0x04FF)],
        "CyrillicSupplement" | "CyrillicSupplementary" => &[(0x0500, 0x052F)],
        "Armenian" => &[(0x0530, 0x058F)],
        "Hebrew" => &[(0x0590, 0x05FF)],
        "Arabic" => &[(0x0600, 0x06FF)],
        "Syriac" => &[(0x0700, 0x074F)],
        "Thaana" => &[(0x0780, 0x07BF)],
        "Devanagari" => &[(0x0900, 0x097F)],
        "Bengali" => &[(0x0980, 0x09FF)],
        "Gurmukhi" => &[(0x0A00, 0x0A7F)],
        "Gujarati" => &[(0x0A80, 0x0AFF)],
        "Oriya" => &[(0x0B00, 0x0B7F)],
        "Tamil" => &[(0x0B80, 0x0BFF)],
        "Telugu" => &[(0x0C00, 0x0C7F)],
        "Kannada" => &[(0x0C80, 0x0CFF)],
        "Malayalam" => &[(0x0D00, 0x0D7F)],
        "Sinhala" => &[(0x0D80, 0x0DFF)],
        "Thai" => &[(0x0E00, 0x0E7F)],
        "Lao" => &[(0x0E80, 0x0EFF)],
        "Tibetan" => &[(0x0F00, 0x0FFF)],
        "Myanmar" => &[(0x1000, 0x109F)],
        "Georgian" => &[(0x10A0, 0x10FF)],
        "HangulJamo" => &[(0x1100, 0x11FF)],
        "Ethiopic" => &[(0x1200, 0x137F)],
        "Cherokee" => &[(0x13A0, 0x13FF)],
        "UnifiedCanadianAboriginalSyllabics" => &[(0x1400, 0x167F)],
        "Ogham" => &[(0x1680, 0x169F)],
        "Runic" => &[(0x16A0, 0x16FF)],
        "Khmer" => &[(0x1780, 0x17FF)],
        "Mongolian" => &[(0x1800, 0x18AF)],
        "LatinExtendedAdditional" => &[(0x1E00, 0x1EFF)],
        "GreekExtended" => &[(0x1F00, 0x1FFF)],
        "GeneralPunctuation" => &[(0x2000, 0x206F)],
        "SuperscriptsandSubscripts" => &[(0x2070, 0x209F)],
        "CurrencySymbols" => &[(0x20A0, 0x20CF)],
        "CombiningMarksforSymbols" | "CombiningDiacriticalMarksforSymbols" => {
            &[(0x20D0, 0x20FF)]
        }
        "LetterlikeSymbols" => &[(0x2100, 0x214F)],
        "NumberForms" => &[(0x2150, 0x218F)],
        "Arrows" => &[(0x2190, 0x21FF)],
        "MathematicalOperators" => &[(0x2200, 0x22FF)],
        "MiscellaneousTechnical" => &[(0x2300, 0x23FF)],
        "ControlPictures" => &[(0x2400, 0x243F)],
        "OpticalCharacterRecognition" => &[(0x2440, 0x245F)],
        "EnclosedAlphanumerics" => &[(0x2460, 0x24FF)],
        "BoxDrawing" => &[(0x2500, 0x257F)],
        "BlockElements" => &[(0x2580, 0x259F)],
        "GeometricShapes" => &[(0x25A0, 0x25FF)],
        "MiscellaneousSymbols" => &[(0x2600, 0x26FF)],
        "Dingbats" => &[(0x2700, 0x27BF)],
        "BraillePatterns" => &[(0x2800, 0x28FF)],
        "CJKRadicalsSupplement" => &[(0x2E80, 0x2EFF)],
        "KangxiRadicals" => &[(0x2F00, 0x2FDF)],
        "IdeographicDescriptionCharacters" => &[(0x2FF0, 0x2FFF)],
        "CJKSymbolsandPunctuation" => &[(0x3000, 0x303F)],
        "Hiragana" => &[(0x3040, 0x309F)],
        "Katakana" => &[(0x30A0, 0x30FF)],
        "Bopomofo" => &[(0x3100, 0x312F)],
        "HangulCompatibilityJamo" => &[(0x3130, 0x318F)],
        "Kanbun" => &[(0x3190, 0x319F)],
        "BopomofoExtended" => &[(0x31A0, 0x31BF)],
        "EnclosedCJKLettersandMonths" => &[(0x3200, 0x32FF)],
        "CJKCompatibility" => &[(0x3300, 0x33FF)],
        "CJKUnifiedIdeographsExtensionA" => &[(0x3400, 0x4DBF)],
        "CJKUnifiedIdeographs" => &[(0x4E00, 0x9FFF)],
        "YiSyllables" => &[(0xA000, 0xA48F)],
        "YiRadicals" => &[(0xA490, 0xA4CF)],
        "HangulSyllables" => &[(0xAC00, 0xD7AF)],
        "HighSurrogates" => &[(0xD800, 0xDB7F)],
        "HighPrivateUseSurrogates" => &[(0xDB80, 0xDBFF)],
        "LowSurrogates" => &[(0xDC00, 0xDFFF)],
        "PrivateUse" => &[(0xE000, 0xF8FF), (0xF0000, 0xFFFFF), (0x100000, 0x10FFFF)],
        "PrivateUseArea" => &[(0xE000, 0xF8FF)],
        "CJKCompatibilityIdeographs" => &[(0xF900, 0xFAFF)],
        "AlphabeticPresentationForms" => &[(0xFB00, 0xFB4F)],
        "ArabicPresentationForms-A" => &[(0xFB50, 0xFDFF)],
        "CombiningHalfMarks" => &[(0xFE20, 0xFE2F)],
        "CJKCompatibilityForms" => &[(0xFE30, 0xFE4F)],
        "SmallFormVariants" => &[(0xFE50, 0xFE6F)],
        "ArabicPresentationForms-B" => &[(0xFE70, 0xFEFE)],
        "Specials" => &[(0xFEFF, 0xFEFF), (0xFFF0, 0xFFFF)],
        "HalfwidthandFullwidthForms" => &[(0xFF00, 0xFFEF)],
        "OldItalic" => &[(0x10300, 0x1032F)],
        "Gothic" => &[(0x10330, 0x1034F)],
        "Deseret" => &[(0x10400, 0x1044F)],
        "ByzantineMusicalSymbols" => &[(0x1D000, 0x1D0FF)],
        "MusicalSymbols" => &[(0x1D100, 0x1D1FF)],
        "MathematicalAlphanumericSymbols" => &[(0x1D400, 0x1D7FF)],
        "CJKUnifiedIdeographsExtensionB" => &[(0x20000, 0x2A6DF)],
        "CJKCompatibilityIdeographsSupplement" => &[(0x2F800, 0x2FA1F)],
        "Tags" => &[(0xE0000, 0xE007F)],
        "SupplementaryPrivateUseArea-A" => &[(0xF0000, 0xFFFFF)],
        "SupplementaryPrivateUseArea-B" => &[(0x100000, 0x10FFFF)],
        _ => return None,
    };
    Some(r)
}

#[cfg(test)]
mod tests {
    use super::XRegex;

    fn m(p: &str, f: &str, s: &str) -> bool {
        XRegex::new(p, f).unwrap().is_match(s).unwrap()
    }

    #[test]
    fn schema_classes() {
        assert!(m("^\\i\\c*$", "", "_a-1.b"));
        assert!(!m("^\\i", "", "1a"));
        assert!(m("^[a-z-[aeiou]]+$", "", "bcd"));
        assert!(!m("^[a-z-[aeiou]]+$", "", "bad"));
        assert!(m("^\\p{IsBasicLatin}+$", "", "abc"));
        assert!(!m("^\\s$", "", "\u{A0}"));
        assert!(!m("^.$", "", "\r"));
        assert!(m("^.$", "s", "\r"));
    }

    #[test]
    fn anchors_flags_backrefs() {
        assert!(!m("^b", "", "a\nb"));
        assert!(m("^b", "m", "a\nb"));
        assert!(m("A B", "xi", "ab"));
        assert!(m("^(a)\\1$", "", "aa"));
        assert!(!m("^(a)\\1$", "", "ab"));
        assert!(m("a.b", "q", "xa.by"));
        assert!(!m("a.b", "q", "axb"));
    }

    #[test]
    fn invalid_patterns() {
        for p in ["a{2,1}", "*a", "\\1(a)", "(", "a]", "\\p{IsNope}", "[z-a]"] {
            let e = XRegex::new(p, "").err().unwrap_or_default();
            assert!(e.starts_with("FORX0002"), "{p}: {e}");
        }
        assert!(XRegex::new("a", "k").is_err());
    }
}
