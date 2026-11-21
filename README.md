# Xylem

Xylem is a fast, standalone structured XML editor written entirely in Rust. It replaces XMetaL XMAX for narrative XML authoring and supports S1000D issues 2.0 to 6.0. It is designed to run on Windows, macOS and Linux: all dependencies are pure Rust. So far it has been built and tested on Windows only.

- **Tags-on and normal views** styled by CSS. It supports the XMetaL CSS dialect, including `$PROCINS`, `$COMMENT`, counters and generated content. A **source view** with syntax highlighting is also available.
- **Schema-guided editing** from XSD or DTD:
  - Only valid elements are offered.
  - Required content is generated, or taken from package templates.
  - Validation runs as you type.
- **Lossless round-trip:** untouched markup is saved byte for byte, including entities, quoting, whitespace and encoding.
- **CALS and HTML tables**, find and replace, spell check (Hunspell), structure tree, attribute inspector and an issues list.
- **S1000D tools:**
  - New data module wizard.
  - DMC and CSDB naming.
  - BREX checking.
  - dmRef navigation.
  - A built-in stylesheet.
- **XMAX migration:** `xylem-import` converts `.ctm` customization folders and `.xac` archives into Xylem doctype packages (see [docs/doctype-packages.md](docs/doctype-packages.md)).
- **Comfortable for long documents:** incremental layout, collapsible elements, a Ctrl+Space insert palette, autosave with crash recovery, and drag-and-drop images.

## Build and run

You need a Rust stable toolchain (1.85 or later).

```
cargo run --release -p xylem-app -- samples/manual-sample.xml
```

To make the sample doctype package available, add its folder to the package path (or use File › Preferences › Add folder):

```
XYLEM_PACKAGES=samples/doctypes cargo run --release -p xylem-app -- samples/manual-sample.xml
```

The binary is `target/release/xylem` (`xylem.exe` on Windows). It has no runtime dependencies.

## Importing XMAX customizations

```
cargo run --release -p xylem-customize --bin xylem-import -- <customization-folder-or.xac> <out-dir>
cargo run --release -p xylem-customize --bin xylem-import -- --all <root> <out-root>
```

Put the output folders in a package folder; the default is `%APPDATA%\Xylem\doctypes` on Windows. Documents are matched to packages by root element, schema location, DOCTYPE public ID or file name. You can also import from inside the editor with File › Import XMAX customization.

## S1000D schemas

Xylem never downloads schemas. To use them, either:

- **Schema folder:** set *File › Preferences › S1000D schema folder* to a folder that contains one sub-folder per issue, for example `S1000D_4-1/…`. `http://www.s1000d.org/S1000D_4-1/...` URLs are then resolved there.
- **Catalog:** point *Schema catalog* at a `catalog.toml` or an OASIS XML catalog.

## Keyboard

| Keys | Action |
|---|---|
| Enter / Shift+Enter | split element (or apply its FollowedBy rule) / insert line break |
| Ctrl+B / I / U | bold / italic / underline toggle (per package) |
| Ctrl+E | select the current element |
| Tab / Shift+Tab | next / previous table cell |
| F5 / F6 / F8 | Tags On / Normal / Source view |
| F7 | validate |
| Ctrl+F | find and replace |
| Ctrl+click | follow a link (IDREF, href, dmRef) |
| Ctrl+Shift+V | paste as plain text |
| Ctrl+Space | insert an element (or surround the selection) |
| Ctrl+Shift+- / = | collapse / expand the current element |
| F3 / Shift+F3 | find next / previous |
| F1 | all keyboard shortcuts |

## Workspace layout

| Crate | Purpose |
|---|---|
| `xylem-dom` | Lossless, editable XML tree (arena, byte-exact serialisation, encodings) |
| `xylem-edit` | Editing engine: caret model, structure-aware commands, tables, undo |
| `xylem-style` | CSS engine (XMetaL dialect, cascade, counters, generated content) |
| `xylem-schema` | XSD and DTD grammars, validation, schema-guided editing queries, catalogs |
| `xylem-xpath` | XPath 1.0 |
| `xylem-s1000d` | S1000D detection, codes, references, BREX, templates |
| `xylem-customize` | Doctype packages and the XMAX importer (`xylem-import`) |
| `xylem-app` | The egui/eframe desktop application (`xylem`) |

## Testing

```
cargo test --workspace
XYLEM_SCHEMA_CORPUS=<folder of real XSDs and instances> cargo test -p xylem-schema --release --test corpus -- --ignored --nocapture
```

The app has a scripted end-to-end harness for UI testing:

```
XYLEM_SCRIPT="click 500 280;;type Hello;;key Enter;;shot out.png;;save out.xml;;quit" xylem file.xml
```

The available steps are `wait N`, `type T`, `paste T`, `drop FILE`, `ctrlclick X Y`, `key NAME [ctrl] [shift] [alt]`, `click X Y`, `dblclick X Y`, `rclick X Y`, `shot PATH`, `save PATH` and `quit`.

See [PARITY.md](PARITY.md) for the XMAX feature checklist.
