# Contributing to Xylem

Thanks for taking an interest. Bug reports, fixes and new features are all
welcome; for anything large, open an issue first so we can agree on the
approach before you spend time on it.

## Getting started

You need a stable Rust toolchain (1.88 or later). On Linux, eframe also needs
the X11/Wayland development headers; the CI workflow lists the packages.

```
cargo build --release -p xylem-app
cargo test --workspace
```

## Before you send a pull request

- `cargo fmt --all` and `cargo clippy --workspace --all-targets` should be
  clean.
- New behaviour comes with tests. Library crates (`xylem-dom`, `xylem-edit`,
  `xylem-schema`, ...) have unit tests next to the code; UI changes can be
  checked with the scripted harness (below).
- If you change what Xylem can do, update [PARITY.md](PARITY.md). Its
  statuses mean what they say: *verified* is for something you have seen
  working in the running app, *tested* for something covered by tests only.
- Keep commits focused, with messages that say what changed and why.

## How the code is laid out

| Crate | What it does |
|-------|--------------|
| `xylem-dom` | Lossless XML document model. Untouched markup is written back byte for byte, so be careful with anything that touches serialisation. |
| `xylem-edit` | The editing engine: caret positions, typing, deleting, splitting, tables, undo. No UI code. |
| `xylem-schema` | XSD and DTD grammars, validation, "what may go here" queries. |
| `xylem-style` | The CSS engine, including XMetaL's extensions. |
| `xylem-xpath` | XPath 1.0, used for BREX checks. |
| `xylem-xslt` | Stylesheet tools: checking output against saved output, timing, analysis, publication preview. |
| `xylem-xslt-engine` | The built-in XSLT 2.0 / XPath 2.0 processor; see [docs/design/xslt-engine.md](docs/design/xslt-engine.md). |
| `xylem-w3c` | Runs the W3C XPath and XSLT test suites against the engine; see [docs/design/conformance.md](docs/design/conformance.md). |
| `xylem-s1000d` | S1000D identification, references, templates, BREX. |
| `xylem-customize` | Doctype packages and the XMAX importer. |
| `xylem-app` | The egui application. |

Keep UI-independent logic out of `xylem-app` where you can: it is much
easier to test in the library crates.

## Testing the app

`scripts/e2e-smoke.sh` drives the real application through a list of
scripted cases and checks the saved XML. The harness takes its steps from the
`XYLEM_SCRIPT` environment variable; `crates/xylem-app/src/script.rs` lists
them. Set `XYLEM_CONFIG_DIR` to a scratch folder so test runs don't touch
your own settings. Click coordinates assume the default window size and the
Windows UI font, so the script currently runs on Windows only.

Some tests need files that can't live in this repository (the S1000D schemas,
real-world schema corpora). They are marked `#[ignore]` and take their input
from environment variables; see the README.

## Licence

By contributing you agree that your work is released under the MIT licence
that covers the project.
