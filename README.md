# Xylem

Xylem is a structured XML editor written in Rust. It was built to replace
XMetaL XMAX for writing narrative XML, and it also handles S1000D (issues 2.0
to 6.0). Everything is pure Rust, so it should run on Windows, macOS and
Linux; so far it has been built and tested on Windows.

What it does:

- Shows the document styled by CSS, either with tags visible ("tags on") or as
  plain formatted text, plus a source view. The XMetaL flavour of CSS that
  XMAX customizations use (`$PROCINS`, `$COMMENT`, counters, generated
  content) works as-is.
- Uses the XSD or DTD to guide editing: only valid elements are offered, new
  elements come with their required content, and validation runs as you type.
- Saves without reformatting: markup you didn't touch is written back byte for
  byte, including entities, quoting, whitespace and encoding.
- Has CALS and HTML tables, find and replace, spell checking with Hunspell
  dictionaries, a structure tree, an attribute inspector and an issues list.
- Tracks changes and keeps review comments in XMetaL's markup, so reviewed
  files round-trip with XMetaL users; see
  [docs/track-changes.md](docs/track-changes.md).
- Finds and replaces across a folder of files (Edit › Find in files). It
  searches text, and attribute values on request, but never tags or
  comments, and rewrites only the matched text in each file.
- Exports a styled, standalone HTML page that looks like the Normal view
  (File › Export to HTML). File › Print preview opens it in your browser to
  print or save as PDF.
- For S1000D: a wizard for new data modules and publication modules, DMC and
  CSDB file naming, BREX checking, dmRef navigation, a built-in stylesheet, a
  project view that lists a CSDB folder and checks the references between its
  modules, and applicability filtering; see [docs/s1000d.md](docs/s1000d.md).
- Converts XMAX customizations (`.ctm` folders and `.xac` archives) into Xylem
  doctype packages with `xylem-import`; see
  [docs/doctype-packages.md](docs/doctype-packages.md).
- Stays quick on long documents (incremental layout, collapsible elements) and
  keeps your work safe (autosave and crash recovery).

## Build and run

You need a Rust stable toolchain (1.88 or later).

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

## Sharing settings with a team

A team can keep its doctype packages, schemas, BREX and dictionaries in one
shared folder and point every installation at it with a small
`xylem-team.toml`. Xylem reads it from `XYLEM_TEAM_CONFIG`, from next to the
executable, or from the config folder; see [docs/team-setup.md](docs/team-setup.md).

## S1000D schemas

Xylem never downloads schemas. Get them from www.s1000d.org (each issue's
`xml_schema_flat` folder, or `xml_schema` for 2.0–2.2) and either put them in
`<config folder>/schemas/s1000d/S1000D_4-1/...`, which Xylem picks up on its
own, or:

- Schema folder: set *File › Preferences › S1000D schema folder* to a folder that contains one sub-folder per issue, for example `S1000D_4-1/…`. `http://www.s1000d.org/S1000D_4-1/...` URLs are then resolved there.
- Catalog: point *Schema catalog* at a `catalog.toml` or an OASIS XML catalog.

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
| Ctrl+Shift+F | find and replace in files |
| Ctrl+click | follow a link (IDREF, href, dmRef) |
| Ctrl+Shift+V | paste as plain text |
| Ctrl+Space | insert an element (or surround the selection) |
| Ctrl+Shift+- / = | collapse / expand the current element |
| Ctrl+Shift+T | track changes on / off |
| Ctrl+Shift+M | new comment |
| Ctrl+Shift+A / R | accept / reject all changes |
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

The available steps are `wait N`, `type T`, `paste T`, `drop FILE`, `ctrlclick X Y`, `key NAME [ctrl] [shift] [alt]`, `click X Y`, `dblclick X Y`, `rclick X Y`, `hover X Y`, `pick PATH` (the answer to the next file dialog), `shot PATH`, `save PATH` and `quit`. `scripts/e2e-smoke.sh` runs the end-to-end cases; see [CONTRIBUTING.md](CONTRIBUTING.md).

Scripted runs never open a web browser: Print preview writes its page to `xylem-print` in the temporary folder and reports the path. Set `XYLEM_NO_BROWSER=1` to get the same behaviour outside the harness.

See [PARITY.md](PARITY.md) for the XMAX feature checklist.

## License

MIT. See [LICENSE](LICENSE).

S1000D schemas are not part of this repository. Point Xylem at your own copy (see "S1000D schemas" above).
