# XMAX parity checklist

This checklist is built from a survey of how our existing authoring host uses
XMetaL XMAX 20: the host application plus its per-customer XMAX
customizations (`.ctm`, `.css`, `.xsd`, `.tbr`, `.js`).

**Scope.** Parity means *authoring* features. Xylem is a standalone editor,
not an ActiveX/COM control, so the XMAX COM object model (events such as
`OnUpdateUI`, `OnContextMenu`, `OnMessage`, properties such as
`ChangedNodes`, `FormattingUpdating`) is out of scope. Where the host used
those APIs to build something users see, that feature is listed instead.

Status values: **verified** means it was exercised in the running app (the
scripted harness plus screenshots); **tested** means implemented and covered
by unit tests but not driven through the app; **partial** and **not done**
mean what they say; **n/a** means the legacy host didn't use it.

**Platforms.** Built and tested on Windows 11. CI for Windows, macOS and
Linux is in `.github/workflows/ci.yml`; it has not run yet, so macOS and Linux
are unverified.

## Document views

| Feature | Status | Notes |
|---|---|---|
| Tags-on view driven by CSS | verified | Sample doctype, built-in S1000D package, and a real 33k-element customer manual with its imported XMAX customization. |
| Normal view (no tags) | verified | |
| Source view | verified | Byte-exact round trip; well-formedness errors with position. |
| Structure tree, with context menu (remove tags, delete, move, collapse) | verified | |
| Collapse and expand elements in the document view | verified | Ctrl+Shift+−/=, the context menu or the structure tree; click the marker to expand. |
| Graphical tables, including spans across columns and rows | verified | CALS (`namest`/`nameend`, `morerows`) and HTML (`colspan`, `rowspan`). |
| Inline images | verified | PNG, JPEG, GIF, BMP, TIFF. CGM and SVG show a labelled box. |
| Images in CSS generated content (`url(...)`) | verified | Drawn before the text in `::before`, after it in `::after`. |
| Comments shown | verified | |
| Zoom, light and dark themes | verified | Both kept across a restart. |

## CSS

| Feature | Status |
|---|---|
| All 90 customer stylesheets parse with no warnings | verified (corpus test) |
| Selectors, generated content, counters, XMetaL `$PROCINS`/`$COMMENT`, `@import` | verified |
| Display types, fonts, colours, boxes, alignment, `white-space`, sup/sub, list markers | verified |
| `wrap-option: emergency` | partial (parsed; long words already wrap) |
| Runtime inline style overrides (`ContainerStyle`) | partial (supported by the engine, no UI) |

## Editing

| Feature | Status | Notes |
|---|---|---|
| Typing, caret and selection; Home/End stop at text, not tag chips | verified | |
| Enter splits; CTM `FollowedBy` | verified | |
| Markup-aware Backspace/Delete, selection delete | verified | |
| Typing between blocks goes into the neighbouring element | verified | |
| Bold/italic/… toggles using the package mapping | verified | |
| Insert from the Elements panel, context menu, or Ctrl+Space palette | verified | Templates from the CTM, otherwise a valid skeleton from the schema. |
| Surround (palette with a selection), split, join, change element | verified | |
| `xm-replace_text` placeholder prompts | verified | |
| Paste plain text and XML fragments with schema checking | verified | |
| Copy and cut XML fragments | verified | Copying a paragraph with `<b>` and `<xref>` and pasting it elsewhere keeps the markup. |
| Undo/redo (journaled; exact dirty state) | verified | |
| Insert comment, PI, CDATA; double-click to edit | verified | |
| Read-only and non-removable elements | verified | Typing and Backspace inside a read-only `<warning>` leave it unchanged. Non-removable is tested only. |
| Protected marker PIs and entity references | tested | The importer protects PIs its stylesheets style (`Ref_Label`, `Entity_Ref_Placeholder`); pass `--protect-pi` for others such as `cms-replace_content`. |
| CALS tables: insert dialog, rows, columns, merge, split, Tab | verified | Insert dialog, Tab, add column, merge down in the app; the rest by unit tests. |
| Automatic IDs on insert | verified | Assigned inside the insert, so one undo removes both. |
| Move elements up/down (keeps whitespace layout) | verified | |
| Follow links with Ctrl+click: IDREF, `href` to another file (with `#id`), S1000D `dmRef` | verified | |
| Drop an image file to insert the package's image element | verified | Wrapped in an allowed container (e.g. a figure) when needed. |

## Validation and tools

| Feature | Status | Notes |
|---|---|---|
| Validation as you type; issues list; invalid tags flagged | verified | |
| Find and replace; F3 / Shift+F3 | verified | |
| Find and replace in files (Ctrl+Shift+F) | verified | Searched a folder with subfolders, opened a result at the match (text and attribute hits), and ran Replace all: an unsaved open file was skipped and reported, a clean one was reloaded, and the rewritten files kept CRLF line ends and character references. Unparseable files are listed, never written, and so is any file the DOM would not write back byte for byte. Read-only elements from a doctype package are not honoured, since files are not matched to packages. |
| Export to HTML | verified | Rendered in headless Edge and Chrome next to the Normal view of the sample manual: counters, list markers, the warning box with its generated image, the CALS table with its spans and the code block all match. A generic document with image elements (one PNG, one missing CGM shown as a labelled box) and an HTML table with `colspan`/`rowspan` matched too. Printed to PDF with backgrounds kept. Vertical spacing is a little tighter than in the editor, because browsers collapse adjacent margins. |
| Print preview (opens the export in the browser) | partial | Writes the page and, in normal use, opens it with the default browser. The browser launch is skipped in scripted runs and was not exercised there. |
| Spell check with suggestions and a user dictionary | verified | With a small local dictionary. No dictionary ships with Xylem; point Preferences at Hunspell `.aff`/`.dic` files. |
| Format document (pretty-print) | verified | Indent amount and per-element "indent content" from the package; mixed content is never touched. |
| Save (lossless, atomic) | verified | |
| Autosave and crash recovery | verified | Killed mid-edit and relaunched: the work was offered back, stayed unsaved until saved, and the copy was removed after saving. |
| Reopen the previous session | verified | |

## Customisation

| Feature | Status |
|---|---|
| `xylem-import` command line (`.ctm` folders and `.xac` archives) | verified (all 137 real sets import and load back) |
| Import from the UI | verified (File menu, choosing the `.ctm` or `.xac`; the imported package is used straight away) |
| Package matching | verified |
| CTM templates, FollowedBy, toggles, lists, images, descriptions, pretty-print, protected PIs | verified |
| `.tbr`, `.js`, `.xdp` | n/a (unused by the legacy host) |

See [docs/doctype-packages.md](docs/doctype-packages.md) for the package format.

## Deployment

XMAX shipped as an ActiveX control inside a host application; these replace
the host's installer and its shared customization folder.

| Feature | Status | Notes |
|---------|--------|-------|
| Shared team configuration (`xylem-team.toml`) | verified | The 137 converted sets load from a shared folder through the team file. |
| Per-user install and uninstall (Start Menu, Installed apps) | verified | Windows. Linux `.desktop` entry is untested. |
| Updates from a team folder | verified | Newer version installed, older never; a half-copied file is refused. |

## S1000D (issues 2.0–6.0)

| Feature | Status | Notes |
|---|---|---|
| Built-in styled rendering | verified | 4.1 descriptive and procedural, and an issue 3.0 descriptive module. |
| New data module wizard | verified | Every issue and kind it offers (52 combinations, 2.0 to 6.0) produces a module that validates against the official schemas (`tests/real_schemas.rs`). |
| BREX check | verified | |
| Data module info; rename to the CSDB file name | verified | |
| dmRef navigation | verified | |
| Validation against the real S1000D schemas | verified | Official schemas for all issues; schema-guided insertion checked in the app on a 4.1 module. |

## Performance (release build, Windows)

| Document | Typing | Enter / insert |
|---|---|---|
| Sample manual (≈100 elements) | < 2 ms | < 5 ms |
| Synthetic manual (20k elements, 1 MB) | ≈ 3 ms | < 25 ms |
| Real customer manual (33k elements, 2.4 MB) | ≈ 14 ms | ≈ 70 ms |

Layout is incremental: a block is laid out again only when its content, its
styles (including counters and generated text) or its box changed. Undo stores
the operations an edit performed, so memory grows with the edits, not with the
document size.
