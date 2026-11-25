# A built-in XSLT 2.0 engine

Status: design and working prototype (`crates/xylem-xslt-engine`).

Xylem's stylesheet tools (Check, Time templates, Analyze, Publication
preview) run stylesheets through `tools/saxon-dotnet`, a .NET program that
hosts Saxon-HE 9.7 the way a production publishing service does. That keeps
Xylem tied to Windows, .NET and a Java-on-.NET runtime. This document plans
the replacement: an XSLT 2.0 / XPath 2.0 processor written in Rust, inside
the workspace, that produces the same bytes.

## Recommendation

**Build it, on our own tree, from the W3C specifications.** The prototype
compiles all seven production publishing stylesheets (the 38 XSLT 2.0
modules) and its output is identical, under the Check tool's own comparison
(which only renumbers the document part of `generate-id()` values), to:

- the saved output of **43 of 43** documents in the four check suites that
  have saved output matching the current stylesheets (Default 5,
  Bulletin 5, Service 31, NarrativeTroubleshooting 2); and
- the live reference host's output for the other three stylesheets on
  83 real inputs each: **249 of 249** (Operator 83, Parts 83,
  NarrativeParts 83).

292 of 292 outputs in total.

The XSLT 1.0 FO-to-HTML stylesheet's HTML, written as its `xsl:output`
asks, is byte-identical to the host's `--serialize` output for 60 of 60
saved FO files. That result, from about 5,800 lines,
is the main evidence that building is feasible and that the remaining work
is mostly matching details rather than missing architecture. Neither
existing crate ran any of the five synthetic cases (below).

Rules for the work:

- Do not read, port or translate Saxon source. Saxon-HE is MPL-2.0, a
  per-file copyleft licence; translated code would be a derived work inside
  an MIT project. Implement from XSLT 2.0, XPath 2.0, XPath Functions and
  Operators 1.0 and XSLT and XQuery Serialization 1.0.
- Saxon (through the host) is used only as a black-box oracle: run the same
  stylesheet and input through both and compare outputs. Where the
  specifications leave behaviour to the implementation, observe what the
  host does from outside (small probe stylesheets; REA on the shipped
  binaries only to see *what* happens, never to copy code) and write it down
  in our own words under "Spec notes" below.

## Existing crates

Both were run over the five synthetic cases in `tests/cases` (`basics`,
`xpath2`, `params`, `numbering`, `imports`; stylesheet given as text,
output compared loosely, ignoring empty-element spelling and quote style):

| Crate | Licence | What it is | Cases matching | What happened |
|---|---|---|---|---|
| `xee-xslt-compiler` 0.1.6 (with `xee-xpath`) | MIT | XPath 3.1 bytecode interpreter over the `xot` tree; early XSLT 3.0 | 0 of 5 | Every case failed to compile (`Unsupported`, and `XPST0017` for a function it lacks). Its AST-to-IR pass (about 900 lines) has no `xsl:call-template`, `xsl:number`, `xsl:key`, `xsl:sort`, `xsl:import`, `xsl:attribute-set` or tunnel parameters; the target stylesheets use all of them (669 `call-template`s, 94 `xsl:number`s, 127 attribute sets, 131 tunnel parameters). |
| `xrust` 2.4.1 | Apache-2.0 | XPath and XSLT over its own tree traits | 0 of 5 | Release build, one case per process. `basics` ran but differed (`last()` wrong, output stops after the first chapter); `xpath2` failed to parse a double literal; `params` rejected `xsl:param` inside a template; `numbering` rejected `xsl:sort` inside `xsl:for-each`; `imports` could not resolve its include because the harness gave no base URL (not a fair test). No tunnel parameters in its source. |

Why not adopt `xee-xpath` for the XPath half only (reasoning, not
measured: only its XSLT layer was run):

- **XPath 3.1 is not XPath 2.0.** The output we must reproduce comes from a
  2.0 processor. Number formatting, `string()` of doubles, error versus
  empty-sequence behaviour and function signatures differ in small ways
  that show up in FO output; fighting a 3.1 engine to behave like 2.0 is
  harder than writing the 2.0 subset the stylesheets use.
- **It brings its own tree (`xot`).** Xylem already has `xylem-dom`; two
  tree models plus conversions in the editor is the cost we would pay.
- **Host integration.** XSLT needs `current()`, `key()`, `document()`,
  `generate-id()` with a specific format, user functions, tunnel parameters
  and per-template timing hooks threaded through evaluation. Owning the
  evaluator makes these direct calls instead of extension points.
- **Size.** The XPath the stylesheets use is mostly XPath 1.0 plus 2.0
  typing, `if`, sequences, `xs:` casts and about 60 functions. In the
  prototype the whole of XPath (parser, evaluator, functions, values) is
  about 2,500 lines.

`xee` remains the reference to check against for XPath 3.1 if Xylem ever
needs 3.0 stylesheets; keep that decision open.

## Survey of the target stylesheets

39 stylesheets, about 16,500 lines; 38 are XSLT 2.0 and one (FO to HTML)
is XSLT 1.0. Counts are occurrences across all modules.

**Instructions used:** `xsl:attribute` 1182, `xsl:when` 763,
`xsl:call-template` 669, `xsl:value-of` 594, `xsl:text` 495,
`xsl:with-param` 483, `xsl:apply-templates` 456, `xsl:template` 422,
`xsl:if` 415, `xsl:variable` 361, `xsl:choose` 304, `xsl:param` 227,
`xsl:attribute-set` 127, `xsl:sequence` 116, `xsl:number` 94,
`xsl:element` 85, `xsl:comment` 50, `xsl:for-each` 49, `xsl:include` 37,
`xsl:sort` 27, `xsl:copy-of` 25, `xsl:output` 18, `xsl:strip-space` 15,
`xsl:key` 13, `xsl:function` 8, `xsl:import` 7, `xsl:message` 2,
`xsl:copy` 2.

**Not used at all:** `xsl:for-each-group`, `xsl:analyze-string`,
`xsl:result-document`, `xsl:next-match`, `xsl:apply-imports`,
`xsl:namespace`, `xsl:character-map`, schema-aware features. They stay out
of scope until something needs them.

**Features:**

- Modes: 266 `mode=` attributes, including `#all` on templates and
  `#current` on `apply-templates`.
- Tunnel parameters: 131.
- `as=` types: `xs:string` 87, `xs:boolean` 44, `item()*` 24,
  `xs:integer` 23, `xs:decimal` 2, `xs:integer?` 1.
- `xsl:number`: `level` single, multiple and any, with `count` and `from`
  patterns; formats `1`, `1.`, `1)`, `(1)`, `A`, `a`, `I`, `i` and their
  punctuated forms, many given as AVTs (`format="{$var}"`).
- `xsl:sort`: 27, all with `select` only (default collation, text order).
- Imports and precedence: 7 imports, 37 includes, explicit `priority` 6
  times; precedence conflicts are resolved by import order.
- `document()` 9 (translation tables, loaded relative to the stylesheet,
  some using an external parsed entity declared in an internal DTD subset).
- XPath 2.0 syntax is used lightly: `if … then … else` a few times, value
  comparisons (`gt`, `lt`), sequences, `xs:` constructor functions. No `for`,
  `some`/`every`, `instance of`, `cast as` or `intersect`/`except` in the
  current stylesheets (the prototype supports them anyway; they are cheap).
- Functions: `not`, `starts-with`, `string-length`, `name`, `generate-id`,
  `count`, `contains`, `concat`, `normalize-space`, `position`, `last`,
  `translate`, `lower-case`, `upper-case`, `ends-with`, `substring*`,
  `key`, `document`, `replace` (7), `matches` (1), `string-join`,
  `distinct-values`, `current`, `number`, `string`, `xs:decimal`,
  `xs:integer`, `xs:string`.
- Regular expressions: 8 uses, all of which run unchanged on the `regex`
  crate in the suites.

## Architecture

```
crates/xylem-xslt-engine
  tree.rs       XDM tree: arena per document, attributes as nodes, resolved names
  value.rs      items and atomic values (string, untypedAtomic, boolean,
                integer, decimal, double), canonical number formatting
  xpath.rs      XPath 2.0 lexer, AST and parser (prefixes resolved at parse time)
  eval.rs       XPath evaluation: paths and axes, predicates, comparisons,
                arithmetic and type promotion, casts, sequence types
  funcs.rs      function library (F&O subset, XSLT functions, xs: constructors)
  compile.rs    stylesheet modules → declarations, template rules, instruction trees
  exec.rs       transformation: template rules and modes, parameters (incl.
                tunnel), variables, instructions, keys, documents, xsl:number,
                xsl:sort, the result builder, per-template timing
  number.rs     xsl:number format tokens, format-number()
  serialize.rs  writing the result the way the publishing host does
```

### Trees: an engine-owned view, not `xylem-dom` directly

`xylem-dom` is an editing model: attributes are not nodes, names are raw
QName strings and there are no namespace bindings, and it keeps source text
for round-tripping. XPath needs attribute nodes, expanded names, a stable
document order across several documents (source, `document()` results,
temporary trees) and node identities for `generate-id()` and keys.

So the engine parses with `xylem-dom` and copies each document once into
its own arena (`tree::Tree`), in document order, with attributes as nodes
and every name resolved. Node references are `(tree, index)`, which gives
document order across trees for free. Whitespace stripping
(`xsl:strip-space`), entity expansion and line-end normalisation happen in
that copy. The copy costs a few milliseconds per document and keeps the
editor's model free of XPath concerns. Result and temporary trees are built
straight into the same arena type.

For the editor this means a preview or check run converts the edited
`Document`; it does not need the editor to change its model.

### Values

Items are nodes or typed atomic values. `xs:decimal` is fixed point with 18
fractional digits (see spec note 3); `xs:integer` is `i64` falling back to
decimal on overflow; `xs:double` is `f64`. Atomization of nodes gives
`xs:untypedAtomic`, which drives the 2.0 comparison and arithmetic rules
(untyped against a number compares as a double, against a string as a
string). Building on `xylem-xpath`'s 1.0 value model would have hidden
exactly the differences the output depends on.

### Patterns and template rules

Patterns are parsed as XPath expressions. A node matches a relative pattern
if evaluating the pattern from one of its ancestors reaches it; absolute
patterns and `key()` patterns are evaluated from the root. A quick test of
the last step's node test rejects most candidates before any evaluation.
Rules per mode are sorted once by import precedence, priority and position
(the last declared wins a tie, as the reference processor does; spec
note 7). This is
fast enough for the production stylesheets (5–200 ms per document in
release builds); a name-indexed rule table is the obvious next step if
needed.

### Hooks for the existing tools

- **Check / Publication preview**: `transform_bytes` returns the result as
  the host writes it (`Output::host_bytes`, UTF-8 with a byte order mark),
  so `xylem-xslt::compare` works unchanged.
- **Time templates**: `transform_bytes(.., time = true)` records calls and
  own and total time per template rule and stylesheet function
  (`Output::timing`), with the file and line the .NET host reports, and the
  built-in `Host` writes them in the host's profile format, so Time
  templates and Analyze work unchanged. Call counts can differ slightly
  from the host's, which optimises some calls away.
- **Analyze**: works on stylesheet source and stays as it is; with the
  engine in-process its proofs can run without starting a process per
  candidate.
- **Errors** carry the template location chain (`in template file#match`)
  so the UI can link them to source; line numbers are the next step
  (`xylem-dom` already records offsets).

### Output

Two serializers are needed:

1. **Host style** (done): what the publishing service writes — the
   `OuterXml` of a .NET `XmlDocument` built from the result: no XML
   declaration, `xsl:output` ignored, empty elements as `<a />`,
   attributes escaping `< > & "` and line feed/tab/CR as character
   references, namespace declarations where a binding first appears.
2. **`xsl:output` style** (to do): the W3C serialization rules for
   `method="xml"`, `"html"` and `"text"`, indentation and doctype, for the
   FO-to-HTML step and for use outside the publishing pipeline.

### XSLT 1.0

The one 1.0 stylesheet (FO to HTML) runs on the host through .NET's own
XSLT 1.0 processor with `xsl:output` serialization, so its oracle is that
processor, not Saxon. The engine keeps a per-stylesheet version flag;
under 1.0 `xsl:value-of` takes the first node (done) and the remaining
backwards-compatible rules (numbers as doubles, 1.0 comparison rules,
first-item semantics for string functions) are a milestone of their own.

## Conformance plan

1. **Oracle suites (primary).** The stylesheet check suites (`*.toml` with
   saved outputs) run through `tests/oracle.rs`'s `suite` test:
   `XYLEM_XSLT_ENGINE_SUITE=<suite.toml> cargo test -p xylem-xslt-engine
   --release --test oracle -- --ignored suite --nocapture`. Where saved
   outputs are out of date, `XYLEM_XSLT_ENGINE_AGAINST_HOST=1` (with
   `XYLEM_SAXON_HOST`) compares with the host's fresh output instead, and
   `XYLEM_XSLT_ENGINE_STYLESHEET` picks the stylesheet. Customer
   documents never enter the repository; only counts and differences are
   reported. The goal is every saved output byte-identical after the usual
   `generate-id` renumbering.
2. **Committed synthetic cases.** `tests/cases/*` hold small stylesheets
   that each pin a behaviour (2.0 vs 1.0 value semantics, number output,
   modes and precedence, tunnel parameters, attribute sets, `xsl:number`,
   keys, sorting, imports). Expected output was produced by the reference
   host and is committed; `live_oracle` re-runs the host when
   `XYLEM_SAXON_HOST` is set (and `XYLEM_XSLT_ENGINE_REGEN=1` rewrites the
   expected files). Every spec note below should get a case.
3. **W3C suites (breadth).** `crates/xylem-w3c` runs the QT3 suite
   (XPath 2.0 subset) and the XSLT test suite (1.0 and 2.0 subset), fetched
   at run time. Repositories, licences, filters and the current pass rates
   are in [conformance.md](conformance.md). Track a pass-rate table per
   feature area; failures in out-of-scope areas (schema awareness,
   `for-each-group` until needed) are expected.
4. **Differential fuzzing (later).** Generate small stylesheet/input pairs
   from the grammar and compare against the host while it is still around.

## Milestones

| | Milestone | Done when |
|---|---|---|
| M0 | Prototype (this branch) | Templates, modes, parameters, variables, choose, for-each, XPath paths and predicates; the oracle suites run end to end |
| M1 | Exact production output | Every saved output in every check suite identical without renumbering ids; the unverified spec notes probed; line numbers in errors |
| M2 | Tools on the engine (done for Check, Time templates, Analyze and Publication preview; `builtin` is the default engine) | Check, Time templates, Analyze proofs and Publication preview can run on the engine, chosen in settings, with the .NET host as the fallback; results compared on every run while both exist |
| M3 | XSLT 1.0 and `xsl:output` (started: FO-to-HTML matches on 60 of 60 files) | FO-to-HTML stylesheet reproduces the host's `--serialize` output; xml/html/text serialization |
| M4 | Breadth | QT3 XPath 2.0 subset ≥ 90 %, XSLT 2.0 subset of xslt30-test tracked; `for-each-group`, `analyze-string`, `result-document`, `next-match`, `apply-imports`, collations |
| M6 | Embedding (started: C library and `xylem-xslt serve`, see [../embedding.md](../embedding.md)) | Another system can replace Saxon with the engine behind its own stylesheet interface |
| M5 | Retire the host (done: the built-in engine is the default and the host is documented as an optional reference) | Default to the engine; the .NET host becomes an optional oracle for development |

## Size estimate

The prototype is about 5,800 lines of Rust. Rough sizes for the rest:

| Area | Lines |
|---|---|
| M1 remaining detail (edge cases found by more suites, errors with lines) | 1,000–2,000 |
| M2 integration in `xylem-xslt` and the app | 1,000–1,500 |
| M3 1.0 compatibility and serialization | 1,500–2,500 |
| M4 breadth (remaining F&O, regex translation, grouping, collations, QT3 runner) | 6,000–10,000 |
| Tests and fixtures | 3,000+ |

So roughly 15,000–25,000 lines for a broadly conformant XSLT 2.0 processor,
of which the part the current tools need (M1–M3) is under a third.

## Spec notes (observed behaviour of the reference processor)

Behaviour the specifications leave open, or where the reference host
differs from a plain reading. Each was found by running probe stylesheets
through the host (or, for note 2, by reading this repository's own host
program); none comes from Saxon source. The fixture that pins each one is
named in brackets; notes marked *unverified* still need a probe.

1. **`generate-id()` format and numbering** [`ids`, `edge`]. IDs are
   `d<document>`, then a letter for the node kind (`e` element, `t` text,
   `c` comment, `p` processing instruction), then the node's number. Nodes
   are numbered in document order from 0 for the document node, without
   attributes or stripped whitespace. Under each parent, once eleven
   children have been numbered since the start or the last skip, the next
   *element* child skips one number (children 12, 23, 34 and so on of an
   all-element parent; with text and comments mixed in, the skip waits for
   the next element). Found by probing parents with 10 to 34 children; it
   explains every gap seen in real documents. Document numbers [`docs`]:
   every module the stylesheet imports or includes takes one before the
   source document does (the top module does not), so with three extra
   modules the source is `d4`; trees made while running (temporary trees,
   `document()` results) follow in the order they are created, global
   variables when first used. A document node's id is just `d<n>`. Ids of
   attributes are not probed yet.
2. **Parameters from the host** [`edge`]. The host (`tools/saxon-dotnet`)
   looks up a parameter's declared type ignoring case but sets the
   parameter under exactly the name given, so a name in different case has
   no effect (its README says otherwise). Values are converted to a
   declared `xs:boolean`, `xs:integer`, `xs:decimal` or `xs:double`;
   anything else, including undeclared types, arrives as `xs:string`, not
   `xs:untypedAtomic`.
3. **Decimal division** [`xpath2`]. `1 div 3` gives `0.333333333333333333`
   (18 fractional digits, truncated); decimal results print without
   trailing zeros (`2.5 * 2` gives `5`, `xs:decimal('1.50')` gives `1.5`);
   `0.1 + 0.2` is exactly `0.3`.
4. **Double to string** [`xpath2`]. Doubles in [1e-6, 1e6) print as plain
   decimals with the shortest round-trip digits (`1e3` gives `1000`,
   `0.000001e0` gives `0.000001`); outside it as `1.0E6` or
   `1.23456789012E11`; `-0.0e0` gives `-0`; `1 div 0e0` gives `INF`.
5. **`xsl:number level="any"` with `from`** [`numbering`]. Counting
   starts at the last node before the numbered node, in document order,
   that matches `from`, and includes every later node matching `count`,
   also outside the `from` node's subtree (the XSLT 1.0 wording; XSLT 2.0's
   formal definition would stop at the subtree). For `level="single"` and
   `"multiple"`, the `from` node may be the numbered node itself [`edge`].
   When no node matches `from`, Saxon (XSLT 2.0) counts from the start of
   the document for `level="any"` and every ancestor for `"multiple"`, as
   if there were no `from`, and gives nothing for `"single"` [`parity`];
   the XSLT 1.0 processor gives nothing at every level [`parity1`].
6. **Empty `xsl:value-of` in a typed variable** [`edge`]. Inside a
   variable with `as="xs:string"` it gives a zero-length string, not an
   empty sequence.
7. **Template conflicts** [`edge`]. Among rules with the same precedence
   and priority the last declared is used, without an error.
8. **Default sort order** (*unverified* beyond [`numbering`]). `xsl:sort`
   without `lang` or `collation` matched code-point order on the cases seen
   (`Bolt, Motor, Seal, Washer`); needs a probe with mixed case and accents.
9. **Serialization** [`basics`]. See "Output" above. A namespace declared
   only on an XSLT instruction (not on a literal result element) is not
   written; `exclude-result-prefixes` applies to literal result elements.
10. **Input normalisation** [`edge`]. Whitespace outside the document
    element and the XML declaration are not part of the tree, and CR LF in
    text arrives as LF. `xylem-dom` keeps all of these as written (and can
    report a declaration after a byte order mark as a processing
    instruction), so the engine's tree copy drops and normalises them.

11. **HTML output of the second step** [`html1`]. The host writes the
    FO-to-HTML result through .NET's XSLT 1.0 processor into a string: the
    doctype uses the root element's name (`<!DOCTYPE html PUBLIC ...>`), a
    `<META http-equiv="Content-Type" content="text/html; charset=utf-16">`
    is inserted as the first child of `head` (utf-16 because the target is
    a string, whatever `encoding` says), void elements have no end tag, `&`
    is escaped in attributes but `<` and `>` are not, non-ASCII characters
    in URI attributes are %-escaped as UTF-8, `script` and `style` content
    is written as is, and line feeds in other text become CR LF. The file
    is then saved as UTF-8 with a byte order mark.
12. **XSLT 1.0 rules** [`html1`]. Numbers are doubles written without an
    exponent (`7 div 3` gives `2.3333333333333335`, `1 div 0` gives
    `Infinity`); arithmetic converts strings with `number()`; comparisons
    follow XPath 1.0 (a boolean on either side compares booleans, ordering
    compares numbers). Setting an attribute that already exists moves it to
    the end in the 1.0 host, while the 2.0 host keeps its place.
    `normalize-space()` removes only space, tab, CR and LF, never a
    no-break space (both hosts).
13. **Output indentation** [`html2`]. The XSLT 1.0 processor indents the
    HTML method unless `indent="no"` is given, and the XML method when
    `indent="yes"` is. A line end (CR LF) and two spaces per open element
    come before a block element's start tag that follows a block start or
    end tag, and before a block end tag that follows a block end tag;
    anything else (text, an inline element) ends the run, so mixed content
    stays on its line. For HTML the block elements are .NET's list (see
    `HTML_BLOCK` in `serialize.rs`; it includes `font`, `s`, `ins` and
    `iframe` but not `script`, `select` or `object`); for XML every element
    is one. A zero-length text node right after a block start tag puts
    the end tag on its own line. The DOCTYPE or XML declaration then has
    a line of its own. A void HTML element given content writes it after
    the start tag and has no end tag. XSLT 2.0 HTML output goes through
    Saxon's serializer, which indents by three spaces with other rules
    (not reproduced).
14. **Which processor runs a stylesheet.** The service runs a stylesheet
    on Saxon when its root `version` attribute is exactly `2.0`, and on
    .NET's XSLT 1.0 processor otherwise (`1.0`, `2`, `3.0`, a simplified
    stylesheet's `xsl:version`); `VersionRule` in `compile.rs`.
15. **XSLT 1.0 result details** [`parity1`]. The result is built through an
    `XmlWriter`: an element's namespace declarations follow its
    attributes; every line end in text (LF, CR or CR LF) is written CR LF;
    a zero-length text node from `value-of` or `copy-of` ends its element
    with a full end tag (`<a></a>`), while an empty `xsl:text` does not.
    The XSLT 2.0 path writes a CR in text as it is. In both, a CR LF in an
    attribute value of the stylesheet or input is one space.
16. **A result that is not a document.** In publish mode the host fails
    on top-level text (even whitespace) and on a second top-level element,
    with its `XmlWriter` message (`Token Text in state Start would result
    in an invalid XML document...`) for XSLT 1.0 and its `XmlDocument`
    message (`The specified node cannot be inserted...`, `This document
    already has a 'DocumentElement' node.`) for XSLT 2.0. Comments,
    processing instructions and an empty result are allowed.
17. **Patterns on parentless nodes** [`parity`]. A one-step pattern
    (`para`, `@id`, `p[@x]`) matches a parentless element or attribute,
    such as one a function built, when its name test and predicates hold;
    a positional predicate (`p[1]`) never does. A multi-step pattern
    (`a/b`) may match its first step at the parentless root of a tree.
18. **Stylesheet text** [`parity`]. Comments and processing instructions
    go before whitespace stripping, so text on either side of one is a
    single text node and keeps its whitespace when any of it is not
    whitespace. Internal entities declared in a stylesheet's (or input's)
    DOCTYPE are expanded in attribute values too, recursively, with
    character references in entity values expanded at declaration.
19. **URIs** [`parity`]. The input's `base-uri()` and `document-uri()` are
    the stylesheet's folder as a `file:///` URI (%20 for spaces, no
    trailing slash); `static-base-uri()` is the calling module's file; a
    temporary tree's base URI is the stylesheet's and it has no document
    URI. `resolve-uri()` results drop an empty authority (`file:/C:/...`)
    and keep the relative part's characters as given. A string passed to
    `document()`, `doc()` or `doc-available()` resolves against the module
    that contains the call.
20. **Unreadable documents.** In XSLT 1.0 a `document()` that cannot be
    read fails the transformation (`An error occurred while loading
    document '...'. See InnerException ... / Could not find file '...'.`).
    In XSLT 2.0 `document()` gives an empty sequence (the engine adds a
    warning) and `doc()` fails. `document('')` is the calling module. A
    malformed document fails in both.
21. **Undeclared variables.** Both processors refuse to compile a
    stylesheet that refers to a variable no declaration provides (XSLT 2.0
    `XPST0008`), even in templates that never run.
22. **Generated IDs in XSLT 1.0** (not reproduced). The 1.0 processor's
    `generate-id()` values have their own form (`ID0EC`, `ID0TH0A`: a
    document number, a node-kind letter and base-32 positions), so they
    differ from the engine's `d1e2` form; they are consistent within a
    result, as links need.
23. **Date pictures.** `format-date` and friends follow Saxon 9.7 where it
    reads the specification its own way: `a.m.`/`p.m.` (`am`/`pm` when the
    width allows under four characters), ordinal words hyphenated
    (`thirty-first`) but cardinals not (`thirty one`), `[F]` in title case
    by default, `[W]` never moving late December into week 1, a maximum
    width truncating the year before the picture's digits pad it
    (`[Y0001,2-2]` gives `0002`), `[f01]` giving exactly two digits, and
    `[Z01]` adding minutes when they are not zero.

## Known gaps in the prototype

- `document()` ignores its second argument (a node whose base URI should
  be used).
- External entities nested inside an external entity resolve against the
  outer document's folder; parameter entities and external DTDs are not
  read.
- `xsl:sort` ignores `case-order`, `lang` and `collation`.
- `format-number()` handles `#`, `0`, `.`, `,` and literal affixes only;
  no percent, per-mille, exponent or `xsl:decimal-format`.
- XSLT 1.0 mode follows the 1.0 processor where the corpus showed it
  (spec notes 12 to 16 and 22); other 1.0 differences, such as
  `format-number` details and the 1.0 processor's `generate-id()` form,
  are not reproduced. XSLT 2.0 `xsl:output` serialization (Saxon's) is
  approximated by the 1.0 processor's.
- Not implemented: `normalize-unicode`, imports inside included modules,
  schema awareness. Namespace declarations made with `xsl:namespace` may
  come out in another order than Saxon writes them. `format-date`,
  `format-dateTime` and `format-time` are English and Gregorian only.
- Errors carry the stylesheet file and line (`XsltError`): compile errors
  the element's line (malformed XML its line and column), run-time errors
  the template and function chain with each declaration's line, not the
  instruction's. Most dynamic errors have no W3C error code.
- The `suite` test reports counts and differences but never fails, so it
  is a measurement, not a gate.

## Open questions

- How `document()` should resolve inside the editor when the edited
  document is unsaved (the host uses the stylesheet's folder as the
  input's base URI; keep that).
- Whether to keep the .NET host as a permanent "compare with reference"
  option for teams that have it.
- Whether Check should keep renumbering generated ids once the engine
  reproduces them exactly (spec note 1).

See also [xsl-fo-formatter.md](xsl-fo-formatter.md) for the separate
question of replacing the FO formatter.
