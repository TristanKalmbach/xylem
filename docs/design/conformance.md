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

| Area | Failures | Notes |
|---|---:|---|
| XML Schema regular expressions | ~815 (XSLT) | Patterns go to the `regex` crate unchanged; XSD syntax (`\i`, `\c`, class subtraction, `$` handling) is not translated |
| Casts and `castable` | ~1,400 (QT3) | Only string, boolean, numeric types; no dates, durations, QNames, `anyURI`, derived integer types |
| `deep-equal`, `compare`, collations | ~200 | Not implemented |
| Numeric functions (`round-half-to-even`, `abs`, `min`, `max`, `sum`, `avg` on mixed types) | ~500 | Type promotion and float/decimal edge cases |
| Dates, times, durations, `format-date` | ~250 | Not implemented |
| `xsl:for-each-group`, `xsl:analyze-string`, `xsl:next-match` | ~100 | Not implemented |
| `format-number`, `use-when`, `base-uri` | ~160 | Partial or missing |

Re-run the commands above after engine changes and update this table.
