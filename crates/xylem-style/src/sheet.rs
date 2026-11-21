//! Style sheets: parsing, `@import` resolution and merging.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::decl::{Declaration, parse_declarations};
use crate::selector::{Selector, parse_selector_list};
use crate::token::{Token, tokenize};
use crate::value::{Cv, components};

/// How many parent folders [`Stylesheet::load`] searches for an import that is
/// not next to the importing file.
const IMPORT_SEARCH_LEVELS: usize = 3;

/// Maximum depth of nested `@import`s (guards against cycles by other means).
const MAX_IMPORT_DEPTH: usize = 16;

/// XMetaL-specific stylesheet switches, written as `@sq-...;` at-rules.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SheetFlags {
    /// `@sq-plus-minus;`
    pub plus_minus: bool,
    /// `@sq-structure-view-icons;`
    pub structure_view_icons: bool,
    /// `@sq-start-only-tags;`
    pub start_only_tags: bool,
}

impl SheetFlags {
    fn merge(&mut self, other: &SheetFlags) {
        self.plus_minus |= other.plus_minus;
        self.structure_view_icons |= other.structure_view_icons;
        self.start_only_tags |= other.start_only_tags;
    }
}

/// The result of resolving an `@import`.
#[derive(Clone, Debug)]
pub struct ResolvedImport {
    /// Identifier of the imported sheet (a path for file-based loading); it is
    /// passed back as the `importer` for that sheet's own imports.
    pub id: String,
    pub text: String,
}

/// One rule: a single selector (comma groups are split) and its declarations.
#[derive(Clone, Debug)]
pub(crate) struct Rule {
    pub selector: Selector,
    pub declarations: Arc<[Declaration]>,
}

/// A parsed style sheet. Rule order is source order, with imports spliced in
/// at the position of their `@import`.
#[derive(Clone, Debug, Default)]
pub struct Stylesheet {
    pub(crate) rules: Vec<Rule>,
    flags: SheetFlags,
    warnings: Vec<String>,
    unresolved_imports: Vec<String>,
}

impl Stylesheet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parses CSS text. `@import`s are not followed (they are listed in
    /// [`Stylesheet::unresolved_imports`]); use
    /// [`Stylesheet::parse_with_resolver`] to follow them.
    pub fn parse(text: &str) -> Stylesheet {
        let mut sheet = Stylesheet::new();
        let mut import_ctx = ImportContext {
            resolver: None,
            stack: Vec::new(),
        };
        sheet.parse_into(text, "<css>", &mut import_ctx);
        sheet
    }

    /// Parses CSS text, resolving `@import`s through `resolver(importer, href)`.
    /// `origin` names `text` (it is the `importer` for its own imports and
    /// prefixes warnings).
    pub fn parse_with_resolver(
        text: &str,
        origin: &str,
        resolver: &mut ImportResolver<'_>,
    ) -> Stylesheet {
        let mut sheet = Stylesheet::new();
        let mut import_ctx = ImportContext {
            resolver: Some(resolver),
            stack: vec![origin.to_string()],
        };
        sheet.parse_into(text, origin, &mut import_ctx);
        sheet
    }

    /// Loads a style sheet from disk, following `@import`s relative to the
    /// importing file (then, leniently, its parent folders). Only a failure to read `path` itself is an error;
    /// unreadable imports become warnings.
    pub fn load(path: &Path) -> std::io::Result<Stylesheet> {
        let text = decode_css(&std::fs::read(path)?);
        let origin = path.to_string_lossy().into_owned();
        let mut resolver = |importer: &str, href: &str| {
            if href.contains("://") {
                return None;
            }
            // XMetaL customizations commonly import a shared sheet that lives
            // in a parent folder, so search upwards a few levels.
            let mut dir = Path::new(importer).parent();
            for _ in 0..=IMPORT_SEARCH_LEVELS {
                let target: PathBuf = dir?.join(href);
                if let Ok(bytes) = std::fs::read(&target) {
                    return Some(ResolvedImport {
                        id: target.to_string_lossy().into_owned(),
                        text: decode_css(&bytes),
                    });
                }
                dir = dir?.parent();
            }
            None
        };
        Ok(Self::parse_with_resolver(&text, &origin, &mut resolver))
    }

    /// Appends `other`'s rules after this sheet's (so they win ties).
    pub fn merge(&mut self, other: Stylesheet) {
        self.rules.extend(other.rules);
        self.flags.merge(&other.flags);
        self.warnings.extend(other.warnings);
        self.unresolved_imports.extend(other.unresolved_imports);
    }

    pub fn flags(&self) -> &SheetFlags {
        &self.flags
    }

    /// Human readable problems found while parsing, as `origin:line: message`.
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// `@import` targets that were not (or could not be) loaded.
    pub fn unresolved_imports(&self) -> &[String] {
        &self.unresolved_imports
    }

    /// Number of (single-selector) rules.
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    fn parse_into(&mut self, text: &str, origin: &str, import_ctx: &mut ImportContext<'_>) {
        let lexed = tokenize(text);
        let tokens = &lexed.tokens;
        let lines = &lexed.lines;
        let mut pos = 0;
        while pos < tokens.len() {
            match &tokens[pos] {
                Token::Whitespace | Token::RBrace | Token::Semicolon => pos += 1,
                Token::AtKeyword(name) => {
                    let line = lines[pos];
                    let (prelude_end, has_block) = find_prelude_end(tokens, pos + 1, true);
                    let prelude = &tokens[pos + 1..prelude_end];
                    let next = if has_block {
                        skip_block(tokens, prelude_end)
                    } else {
                        prelude_end + 1
                    };
                    self.at_rule(name, prelude, has_block, (origin, line), import_ctx);
                    pos = next;
                }
                _ => {
                    let line = lines[pos];
                    let (prelude_end, has_block) = find_prelude_end(tokens, pos, false);
                    if !has_block {
                        self.warn(origin, line, "rule without a declaration block ignored");
                        break;
                    }
                    let block_end = skip_block(tokens, prelude_end);
                    let closed =
                        block_end > prelude_end + 1 && tokens[block_end - 1] == Token::RBrace;
                    let body_end = if closed { block_end - 1 } else { block_end };
                    self.rule(origin, &lexed, pos..prelude_end, prelude_end + 1..body_end);
                    pos = block_end;
                }
            }
        }
    }

    fn rule(
        &mut self,
        origin: &str,
        lexed: &crate::token::Tokens,
        selector: std::ops::Range<usize>,
        body: std::ops::Range<usize>,
    ) {
        let line = lexed.lines[selector.start];
        let selectors = match parse_selector_list(&lexed.tokens[selector]) {
            Ok(s) => s,
            Err(message) => {
                self.warn(origin, line, &format!("rule dropped: {message}"));
                return;
            }
        };
        let mut problems = Vec::new();
        let declarations: Arc<[Declaration]> = parse_declarations(
            &lexed.tokens[body.clone()],
            &lexed.lines[body],
            &mut |l, m| problems.push((l, m)),
        )
        .into();
        for (l, m) in problems {
            self.warn(origin, l, &m);
        }
        if declarations.is_empty() {
            return;
        }
        self.rules
            .extend(selectors.into_iter().map(|selector| Rule {
                selector,
                declarations: Arc::clone(&declarations),
            }));
    }

    fn at_rule(
        &mut self,
        name: &str,
        prelude: &[Token],
        has_block: bool,
        (origin, line): (&str, u32),
        import_ctx: &mut ImportContext<'_>,
    ) {
        match name.to_ascii_lowercase().as_str() {
            "sq-plus-minus" => self.flags.plus_minus = true,
            "sq-structure-view-icons" => self.flags.structure_view_icons = true,
            "sq-start-only-tags" => self.flags.start_only_tags = true,
            "namespace" | "charset" => {}
            "import" if !has_block => self.import(prelude, (origin, line), import_ctx),
            other => self.warn(
                origin,
                line,
                &format!("unsupported at-rule `@{other}` skipped"),
            ),
        }
    }

    fn import(
        &mut self,
        prelude: &[Token],
        (origin, line): (&str, u32),
        import_ctx: &mut ImportContext<'_>,
    ) {
        let href = match components(prelude).first() {
            Some(Cv::Str(s) | Cv::Url(s)) => s.clone(),
            Some(Cv::Function(name, args)) if name == "url" => match args.as_slice() {
                [Cv::Str(s)] => s.clone(),
                _ => String::new(),
            },
            _ => String::new(),
        };
        if href.is_empty() {
            self.warn(origin, line, "@import without a target ignored");
            return;
        }
        let Some(resolver) = import_ctx.resolver.as_deref_mut() else {
            self.unresolved_imports.push(href);
            return;
        };
        if import_ctx.stack.len() > MAX_IMPORT_DEPTH {
            self.warn(
                origin,
                line,
                &format!("@import of `{href}` nested too deeply"),
            );
            return;
        }
        let Some(imported) = resolver(origin, &href) else {
            self.warn(
                origin,
                line,
                &format!("@import of `{href}` could not be loaded"),
            );
            self.unresolved_imports.push(href);
            return;
        };
        if import_ctx.stack.contains(&imported.id) {
            self.warn(
                origin,
                line,
                &format!("@import cycle through `{}` ignored", imported.id),
            );
            return;
        }
        import_ctx.stack.push(imported.id.clone());
        self.parse_into(&imported.text, &imported.id, import_ctx);
        import_ctx.stack.pop();
    }

    fn warn(&mut self, origin: &str, line: u32, message: &str) {
        self.warnings.push(format!("{origin}:{line}: {message}"));
    }
}

/// Resolves `(importer, href)` to the imported sheet.
type ImportResolver<'a> = dyn FnMut(&str, &str) -> Option<ResolvedImport> + 'a;

struct ImportContext<'a> {
    resolver: Option<&'a mut ImportResolver<'a>>,
    /// Ids of the sheets currently being parsed, outermost first.
    stack: Vec<String>,
}

/// Finds the end of a rule prelude starting at `from`: the index of the `{`
/// (block follows) or of the `;` (when `stop_at_semicolon`), or the token
/// count at EOF. Returns `(index, has_block)`.
fn find_prelude_end(tokens: &[Token], from: usize, stop_at_semicolon: bool) -> (usize, bool) {
    let mut depth = 0usize;
    for (i, token) in tokens.iter().enumerate().skip(from) {
        match token {
            Token::LParen | Token::LBracket | Token::Function(_) => depth += 1,
            Token::RParen | Token::RBracket => depth = depth.saturating_sub(1),
            Token::LBrace if depth == 0 => return (i, true),
            Token::Semicolon if depth == 0 && stop_at_semicolon => return (i, false),
            _ => {}
        }
    }
    (tokens.len(), false)
}

/// Given the index of a `{`, returns the index just past its matching `}`
/// (or the token count if unterminated).
fn skip_block(tokens: &[Token], open: usize) -> usize {
    let mut depth = 0usize;
    for (i, token) in tokens.iter().enumerate().skip(open) {
        match token {
            Token::LBrace => depth += 1,
            Token::RBrace => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
    }
    tokens.len()
}

/// Decodes stylesheet bytes: UTF-8 (BOM stripped), UTF-16 with BOM, else Latin-1.
pub(crate) fn decode_css(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        let little_endian = bytes[0] == 0xFF;
        let (pairs, _) = bytes[2..].as_chunks::<2>();
        let units = pairs.iter().map(|&pair| {
            if little_endian {
                u16::from_le_bytes(pair)
            } else {
                u16::from_be_bytes(pair)
            }
        });
        return char::decode_utf16(units)
            .map(|r| r.unwrap_or('\u{fffd}'))
            .collect();
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn rules_are_split_by_comma_and_share_declarations() {
        let sheet = Stylesheet::parse("a, b > c { display: block; color: red }");
        assert_eq!(sheet.rule_count(), 2);
        assert!(sheet.warnings().is_empty(), "{:?}", sheet.warnings());
    }

    #[test]
    fn flags_namespace_and_unknown_at_rules() {
        let sheet = Stylesheet::parse(
            "@namespace url(http://x);\n@sq-plus-minus;\n@sq-structure-view-icons;\n@media print { a { color: red } }\n@sq-start-only-tags;\na { display: block }",
        );
        assert_eq!(
            sheet.flags(),
            &SheetFlags {
                plus_minus: true,
                structure_view_icons: true,
                start_only_tags: true
            }
        );
        assert_eq!(sheet.rule_count(), 1);
        assert_eq!(sheet.warnings().len(), 1);
        assert!(sheet.warnings()[0].contains("@media"));
    }

    #[test]
    fn bad_rules_do_not_poison_following_rules() {
        let sheet = Stylesheet::parse(
            "a:hover { color: red }\nb { frob: 1; color: blue }\nc { display: block }",
        );
        assert_eq!(sheet.rule_count(), 2);
        assert_eq!(sheet.warnings().len(), 2);
        assert!(sheet.warnings()[0].contains(":1:"));
    }

    #[test]
    fn unterminated_input_is_tolerated() {
        let sheet = Stylesheet::parse("a { display: block; b { color: red");
        let _ = sheet.rule_count();
        let sheet = Stylesheet::parse("a { display: block");
        assert_eq!(sheet.rule_count(), 1);
    }

    #[test]
    fn imports_are_spliced_in_order_and_resolved_relative() {
        let mut files: HashMap<&str, &str> = HashMap::new();
        files.insert(
            "dir/common.css",
            "@import \"sub/inner.css\"; a { color: red }",
        );
        files.insert("dir/sub/inner.css", "@namespace x; b { color: blue }");
        let mut seen = Vec::new();
        let mut resolver = |importer: &str, href: &str| {
            let base = importer.rsplit_once('/').map_or("", |(d, _)| d);
            let id = if base.is_empty() {
                href.to_string()
            } else {
                format!("{base}/{href}")
            };
            seen.push(id.clone());
            files.get(id.as_str()).map(|t| ResolvedImport {
                id: id.clone(),
                text: (*t).to_string(),
            })
        };
        let sheet = Stylesheet::parse_with_resolver(
            "@import url(\"common.css\");\n@import url(missing.css);\nc { color: green }",
            "dir/main.css",
            &mut resolver,
        );
        assert_eq!(
            seen,
            ["dir/common.css", "dir/sub/inner.css", "dir/missing.css"]
        );
        assert_eq!(sheet.rule_count(), 3);
        assert_eq!(sheet.unresolved_imports(), ["missing.css"]);
        assert!(
            sheet
                .warnings()
                .iter()
                .any(|w| w.starts_with("dir/main.css:2:"))
        );
    }

    #[test]
    fn import_cycles_terminate() {
        let mut resolver = |_: &str, href: &str| {
            Some(ResolvedImport {
                id: href.to_string(),
                text: format!("@import \"{href}\"; a {{ color: red }}"),
            })
        };
        let sheet =
            Stylesheet::parse_with_resolver("@import \"self.css\";", "main.css", &mut resolver);
        assert_eq!(sheet.rule_count(), 1);
        assert!(sheet.warnings().iter().any(|w| w.contains("cycle")));
    }

    #[test]
    fn parse_without_resolver_records_imports() {
        let sheet = Stylesheet::parse("@import 'x.css'; @import url(y.css);");
        assert_eq!(sheet.unresolved_imports(), ["x.css", "y.css"]);
    }

    #[test]
    fn merge_appends_rules_and_ors_flags() {
        let mut a = Stylesheet::parse("@sq-plus-minus; a { color: red }");
        a.merge(Stylesheet::parse("@sq-start-only-tags; b { color: blue }"));
        assert_eq!(a.rule_count(), 2);
        assert!(
            a.flags().plus_minus && a.flags().start_only_tags && !a.flags().structure_view_icons
        );
    }

    #[test]
    fn decoding() {
        assert_eq!(decode_css(b"\xEF\xBB\xBFa"), "a");
        assert_eq!(decode_css(&[0xFF, 0xFE, b'a', 0]), "a");
        assert_eq!(decode_css(b"caf\xE9"), "caf\u{e9}");
    }
}
