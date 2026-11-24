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
already runs the full production publishing stylesheet (38 modules) and
reproduces the saved output of **43 of 43** documents in the four check
suites tried, identical apart from `generate-id()` values (see spec note 1).
That result, from about 5,700 lines written in a few days, is the main
evidence that building is feasible and that the work is mostly about
matching details rather than missing architecture.

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

| Crate | Licence | What it is | Fit |
|---|---|---|---|
| `xee` (`xee-xpath`, `xee-xslt-compiler` 0.1.6) | MIT | XPath 3.1 (bytecode interpreter over the `xot` tree), early XSLT 3.0 | XSLT layer far too small: the AST-to-IR pass (≈900 lines) has no `xsl:call-template`, `xsl:number`, `xsl:key`, `xsl:sort`, `xsl:import`, `xsl:attribute-set` or tunnel parameters. The target stylesheets use all of them (669 `call-template`s, 94 `xsl:number`s, 127 attribute sets, 131 tunnel parameters). |
| `xrust` 2.4.1 | Apache-2.0 | XPath and XSLT over its own tree traits | Implements a subset of XSLT 1.0/2.0 instructions; no tunnel parameters, a different tree model, and its value model follows its own conventions rather than XDM typing. Adopting it means rewriting most of it to get byte-exact output. |

Why not adopt `xee-xpath` for the XPath half only:

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
- Regular expressions: 8 uses, all in the common subset of XML Schema regex
  syntax and the `regex` crate's.

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
(the last declared wins a tie, as the reference processor does). This is
fast enough for the production stylesheets (5–200 ms per document in
release builds); a name-indexed rule table is the obvious next step if
needed.

### Hooks for the existing tools

- **Check / Publication preview**: `transform_bytes` returns the result as
  the host writes it (`Output::host_bytes`, UTF-8 with a byte order mark),
  so `xylem-xslt::compare` works unchanged.
- **Time templates**: `transform_bytes(.., time = true)` records calls and
  wall time per template rule (`Output::timing`), keyed by the rule's file
  and name/match — the same granularity the host's profile gives.
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
   --release --test oracle -- --ignored suite --nocapture`. Customer
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
3. **W3C suites (breadth).** QT3 (`w3c/qt3tests`, W3C test suite licence)
   covers XPath 2.0 and F&O; filter to `spec="XP20"` and the functions in
   scope. The XSLT suite (`w3c/xslt30-test`) covers XSLT 2.0 when filtered
   by its `spec` dependencies to `XSLT20+`. Fetch both at test time into a
   cache directory; do not vendor them. Track a pass-rate table per
   feature area; failures in out-of-scope areas (schema awareness,
   `for-each-group` until needed) are expected.
4. **Differential fuzzing (later).** Generate small stylesheet/input pairs
   from the grammar and compare against the host while it is still around.

## Milestones

| | Milestone | Done when |
|---|---|---|
| M0 | Prototype (this branch) | Templates, modes, parameters, variables, choose, for-each, XPath paths and predicates; the oracle suites run end to end |
| M1 | Exact production output | Every saved output in all check suites identical, including `generate-id` numbering (spec note 1); line numbers in errors |
| M2 | Tools on the engine | Check, Time templates, Analyze proofs and Publication preview can run on the engine, chosen in settings, with the .NET host as the fallback; results compared on every run while both exist |
| M3 | XSLT 1.0 and `xsl:output` | FO-to-HTML stylesheet reproduces the host's `--serialize` output; xml/html/text serialization |
| M4 | Breadth | QT3 XPath 2.0 subset ≥ 90 %, XSLT 2.0 subset of xslt30-test tracked; `for-each-group`, `analyze-string`, `result-document`, `next-match`, `apply-imports`, collations |
| M5 | Retire the host | Default to the engine; the .NET host becomes an optional oracle for development |

## Size estimate

The prototype is about 5,700 lines of Rust. Rough sizes for the rest:

| Area | Lines |
|---|---|
| M1 remaining detail (ids, edge cases found by suites, errors with lines) | 1,000–2,000 |
| M2 integration in `xylem-xslt` and the app | 1,000–1,500 |
| M3 1.0 compatibility and serialization | 1,500–2,500 |
| M4 breadth (remaining F&O, regex translation, grouping, collations, QT3 runner) | 6,000–10,000 |
| Tests and fixtures | 3,000+ |

So roughly 15,000–25,000 lines for a broadly conformant XSLT 2.0 processor,
of which the part the current tools need (M1–M3) is under a third.

## Spec notes (observed behaviour of the reference processor)

Each note is behaviour the specifications leave open, or where the
reference host differs from a plain reading, found by running probe
stylesheets through the host and comparing. They are written from
observation, not from Saxon source.

1. **`generate-id()` format and numbering.** IDs are `d<doc>` followed by
   a letter for the node kind (`e` element, `t` text, `c` comment, `p`
   processing instruction) and the node's number. Nodes are numbered in
   document order from 0 for the document node, attributes and stripped
   whitespace excluded. On real inputs the host occasionally skips a
   number (6 gaps in a 213-node document, at irregular places, always
   just before an element following whitespace that was stripped). The
   cause is not yet known — the input is read through the .NET XML reader
   before reaching the processor, so it may be how stripped whitespace
   arrives in pieces. Open question for M1: observe the host's tree builder
   (REA on the shipped assemblies) to find the rule, then write a probe
   case for it. The engine currently numbers without gaps; Check's
   comparison renumbers only the document part, so these documents show
   id-only differences.
2. **Parameters from the host.** Stylesheet parameters given on the host's
   command line are matched to global `xsl:param`s and converted to the
   declared type (`as="xs:integer"` receives an integer). The host's
   documentation says the match ignores case; a probe passing `PAGECOUNT`
   for `pageCount` did not take effect, so this needs checking before M2.
   The engine currently ignores case.
3. **Decimal division.** `1 div 3` gives `0.333333333333333333` (18
   fractional digits, truncated); decimal results print without trailing
   zeros (`2.5 * 2` → `5`, `xs:decimal('1.50')` → `1.5`). `0.1 + 0.2` is
   exactly `0.3`. The engine's fixed-point decimal follows this.
4. **Double to string.** Doubles in [1e-6, 1e6) print as plain decimals
   with the shortest round-trip digits (`1e3` → `1000`, `0.000001e0` →
   `0.000001`); outside it as `1.0E6`, `1.23456789012E11`; `-0.0e0` →
   `-0`; `1 div 0e0` → `INF`.
5. **`xsl:number level="any"` with `from`.** Counting starts at the last
   node before the numbered node (in document order) that matches `from`,
   and includes every later node matching `count` — also nodes outside the
   `from` node's subtree. This is the XSLT 1.0 wording; XSLT 2.0's formal
   definition would restrict counting to the subtree. For `level="single"`
   and `"multiple"`, the `from` node may be the numbered node itself.
6. **`xsl:value-of` producing an empty string** inside a variable with
   `as="xs:string"` gives a zero-length string, not an empty sequence.
7. **Template conflicts.** Among rules with the same precedence and
   priority, the one declared last wins (no error).
8. **Default sort order.** `xsl:sort` without `lang` or `collation` sorts
   consistently with Unicode code point order on the cases seen so far
   (`Bolt, Motor, Seal, Washer`). The engine sorts by code point. Needs a
   probe with mixed case and accents before M1.
9. **Serialization.** See "Output" above; also: a namespace declared only
   on an XSLT instruction (not on a literal result element) is not written,
   and `exclude-result-prefixes` applies to literal result elements only.
10. **Document-level whitespace and line ends.** Whitespace outside the
    document element is not part of the tree; CR LF in text arrives as LF.
    `xylem-dom` keeps both as written, so the engine's tree copy drops and
    normalises them.

## Open questions

- How `document()` should resolve inside the editor when the edited
  document is unsaved (the host uses the stylesheet's folder as the
  input's base URI; keep that).
- Whether to keep the .NET host as a permanent "compare with reference"
  option for teams that have it.
- Spec note 1 decides whether Check can require byte-identical ids or
  keeps renumbering.

See also [xsl-fo-formatter.md](xsl-fo-formatter.md) for the separate
question of replacing the FO formatter.
