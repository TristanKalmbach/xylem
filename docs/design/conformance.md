# W3C conformance of the built-in engine

`crates/xylem-w3c` runs the W3C test suites against Xylem's built-in XSLT
engine (`crates/xylem-xslt-engine`) and reports pass rates per test set.

```
cargo run --release -p xylem-w3c -- fetch            # shallow-clones both suites into target/w3c
cargo run --release -p xylem-w3c -- qt3  --out qt3.md
cargo run --release -p xylem-w3c -- xslt --out xslt.md
```

`--set <prefix>` runs only matching test sets and `--show <n>` lists the
first failures. Each test set runs in its own process, so a crash or hang
in one set (limit: 2 minutes) does not stop the run.

## The suites and their licences

| Suite | Repository | What is run |
|---|---|---|
| QT3 | [w3c/qt3tests](https://github.com/w3c/qt3tests) | Tests whose spec dependency includes XPath 2.0 (`XP20`, `XP20+`) or that have none |
| XSLT | [w3c/xslt30-test](https://github.com/w3c/xslt30-test) | Tests whose spec dependency includes XSLT 1.0 or 2.0 (`XSLT10`, `XSLT10+`, `XSLT20`, `XSLT20+`) |

Neither repository carries a licence file or statement (checked
2026-11-24). W3C's test-suite policy
(<https://www.w3.org/copyright/test-suites-licenses/>) offers test suites
under the W3C 3-clause BSD licence for development use and under the W3C
test suite licence for conformance claims, but says it applies to a suite
once that suite is updated to carry the notice. So:

- the suites are **fetched at run time** and never copied into this
  repository or its releases;
- they are used for development only, and Xylem makes **no conformance
  claim** from them.

## Filters

A test is skipped, and counted as skipped with its reason, when it needs
something outside the engine's scope: schema awareness, optional features
(`namespace-axis`, `staticTyping`, `serialization`, `dtd`, ...), XML 1.1,
collections, resources, sources other than the context item, parameters
computed by expressions, inline or secondary stylesheets, initial
functions, or assertion kinds the runner does not check
(`serialization-matches`, `assert-result-document`, `assert-message`, ...).
A dependency on a Unicode version, or on XSLT 1.0 compatibility mode, does
not cause a skip.

Results are compared the suites' way where the runner can: values with
XPath (`eq`, `instance of`, assertions over `$result`), XML results after
parsing both sides and writing them back the engine's way, and any error
where an error is expected (error codes are not compared: the engine does
not report W3C error codes yet).

## Baseline (2026-11-24)

| Suite | Run | Passed | Rate | Skipped |
|---|---:|---:|---:|---:|
| QT3, XPath 2.0 | 14,956 | 8,711 | 58.2% | 16,758 |
| XSLT 1.0 and 2.0 | 4,682 | 2,719 | 58.1% | 7,728 |

These are the first numbers for an engine built to reproduce a set of
production stylesheets (which it does exactly), not general conformance.
The largest groups of failures point at what to build next:

## Result documents and the type system (2026-11-24)

Added: `xsl:result-document` (secondary results in `Output::results`,
written below the principal output's folder by the CLI and host; the C
API refuses them), `deep-equal`, `doc-available`, `round-half-to-even`,
`remove`, `insert-before`, `unordered`, `compare`, `codepoint-equal`,
`encode-for-uri`, `iri-to-uri`, `escape-html-uri`, the QName functions,
and a type layer: `src/datetime.rs` (all date/time types and durations:
casts, canonical forms, comparison, arithmetic, component and timezone
functions, `current-*`; the implicit timezone is UTC) and `src/types.rs`
(derived integer and string types, `anyURI`, `float`, the type hierarchy
for `instance of`). XPath 3.0 regex syntax (`(?:...)`, the `q` flag) is
rejected for stylesheets below version 3.0. The XSLT runner checks
`assert-result-document`.

| Suite | Run | Passed | Rate |
|---|---:|---:|---:|
| QT3, XPath 2.0 | 15,054 | 14,045 | 93.3% |
| XSLT 1.0 and 2.0 | 4,682 | 3,716 | 79.4% |

Then `xsl:next-match`, `xsl:apply-imports`, `xsl:for-each-group` (with
`current-group`, `current-grouping-key`), `xsl:perform-sort`,
`xsl:document` and simplified stylesheets: XSLT 3,858 of 4,682 (82.5%).

Then `xs:hexBinary` and `xs:base64Binary`, casts to `xs:QName` from string
literals (prefixes resolved statically), XPST0080 for casts to
`xs:NOTATION` and `xs:anyAtomicType`, `xs:anyURI` escape checks, `xs:float`
values that keep their type (float-precision comparison, exact decimal and
float `round`, `floor`, `ceiling`, `abs`, `round-half-to-even`), and
`min` / `max` that promote to the widest numeric type: QT3 14,587 of 15,054
(96.9%), XSLT 3,886 of 4,682 (83.1%).

| Set | Now |
|---|---:|
| `regex-syntax-xslt20` | 984 / 987 (3 depend on the Unicode version of `\w`, `\d`, `\c`) |
| `regex-classes` | 89 / 120 (the rest compare against older Unicode tables) |

The largest remaining groups:

| Area | Failures | Notes |
|---|---:|---|
| Wrong output (`got ...`) | ~400 each suite | Mixed: number formatting, serialization details, edge cases |
| Expected errors not raised | ~210 (QT3), ~100 (XSLT) | Type checks the engine does not make yet |
| Type annotations in patterns and sequence types (`element(*, xs:untyped)`) | some (XSLT) | Not parsed |
| `base-uri`, `resolve-uri`, `static-base-uri`, `document-uri` | ~90 | Base URIs are file paths, not URIs |
| `format-date`, `format-dateTime`, `format-time` | ~30 (XSLT) | Not implemented |
| `xsl:namespace` | ~10 (XSLT) | Not implemented |
| `normalize-unicode`, `trace`, `lang` | ~70 (QT3) | Not implemented |
| Sets that crash | `function`, `call-template`, `error` (stack overflow on deep recursion), `unicode-90` (timeout) | Not counted in the totals |

Re-run the commands above after engine changes and update this table.

## After XML Schema regex translation (2026-11-24)

`src/xsd_regex.rs` parses XPath / XML Schema regex syntax and translates
it for the `regex` crate: `\i \I \c \C`, `\s \d \w` with schema meanings,
class subtraction, `\p{Is...}` block names, `.` excluding CR/LF, string
anchors, the `s m i x q` flags, and `FORX0002` for invalid patterns.
Patterns with back-references run on `fancy-regex`. `xsl:analyze-string`,
`regex-group`, `string-to-codepoints` and `codepoints-to-string` were
added, and the XSLT runner now takes an assertion's effective boolean
value (it previously required an `xs:boolean`, failing ~410 regex tests
that had passed).

| Suite | Run | Passed | Rate |
|---|---:|---:|---:|
| QT3, XPath 2.0 | 14,956 | 8,862 | 59.3% |
| XSLT 1.0 and 2.0 | 4,682 | 3,427 | 73.2% |

| Set | Before | After |
|---|---:|---:|
| `regex-syntax-xslt20` | 292 / 987 | 946 / 987 |
| `regex` | 35 / 49 | 48 / 49 |
| `fn-matches` | | 145 / 150 |
| `fn-replace` | | 79 / 81 |
| `fn-tokenize` | | 43 / 49 |
| `regex-classes` | 0 / 120 | 0 / 120 (then blocked on `xsl:result-document`; see below) |

(Superseded below: `(?:...)` is now rejected for stylesheets below 3.0.)

