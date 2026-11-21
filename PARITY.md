# XMAX parity checklist

This checklist is built from a survey of how our existing authoring host uses
XMetaL XMAX 20: the host application plus its per-customer XMAX
customizations (`.ctm`, `.css`, `.xsd`, `.tbr`, `.js`).

**Scope.** Parity means *authoring* features. Xylem is a standalone editor,
not an ActiveX/COM control, so the XMAX COM object model (events such as
`OnUpdateUI`, `OnContextMenu`, `OnMessage`, properties such as
`ChangedNodes`, `FormattingUpdating`) is out of scope. Where the host used
those APIs to build something users see, that feature is listed instead.

**Legend.**
✅ verified in the running app (scripted end-to-end harness plus screenshots) ·
☑️ implemented and unit-tested, not exercised in the app ·
🟡 partial · ⬜ not done · ➖ not applicable

**Platforms.** Built and tested on Windows 11. CI is set up for Windows,
macOS and Linux (`.github/workflows/ci.yml`) but has not run yet.

## Document views

| Feature | Status | Notes |
|---|---|---|
| Tags-on view driven by CSS | ✅ | Sample doctype, built-in S1000D package, and a real 33k-element customer manual with its imported XMAX customization. |
| Normal view (no tags) | ✅ | |
| Source view | ✅ | Byte-exact round trip; well-formedness errors with position. |
| Structure tree, with context menu (remove tags, delete, move, collapse) | ✅ | |
| Collapse and expand elements in the document view | ✅ | Ctrl+Shift+−/=, the context menu or the structure tree; click the marker to expand. |
| Graphical tables, including spans across columns and rows | ✅ | CALS (`namest`/`nameend`, `morerows`) and HTML (`colspan`, `rowspan`). |
| Inline images | ✅ | PNG, JPEG, GIF, BMP, TIFF. CGM and SVG show a labelled box. |
| Images in CSS generated content (`url(...)`) | ✅ | Drawn before the text in `::before`, after it in `::after`. |
| Comments shown | ✅ | |
| Zoom, light and dark themes | ☑️ | |

## CSS

| Feature | Status |
|---|---|
| All 90 customer stylesheets parse with no warnings | ✅ (corpus test) |
| Selectors, generated content, counters, XMetaL `$PROCINS`/`$COMMENT`, `@import` | ✅ |
| Display types, fonts, colours, boxes, alignment, `white-space`, sup/sub, list markers | ✅ |
| `wrap-option: emergency` | 🟡 (parsed; egui already breaks long words) |
| Runtime inline style overrides (`ContainerStyle`) | 🟡 (supported by the engine, no UI) |

## Editing

| Feature | Status | Notes |
|---|---|---|
| Typing, caret and selection; Home/End stop at text, not tag chips | ✅ | |
| Enter splits; CTM `FollowedBy` | ✅ | |
| Markup-aware Backspace/Delete, selection delete | ✅ | |
| Typing between blocks goes into the neighbouring element | ✅ | |
| Bold/italic/… toggles using the package mapping | ✅ | |
| Insert from the Elements panel, context menu, or Ctrl+Space palette | ✅ | Templates from the CTM, otherwise a valid skeleton from the schema. |
| Surround (palette with a selection), split, join, change element | ✅ | |
| `xm-replace_text` placeholder prompts | ✅ | |
| Paste plain text and XML fragments with schema checking | ✅ | |
| Copy and cut XML fragments | ☑️ | |
| Undo/redo (journaled; exact dirty state) | ✅ | |
| Insert comment, PI, CDATA; double-click to edit | ✅ | |
| Read-only and non-removable elements | ☑️ | |
| Protected marker PIs (`Ref_Label`, `Entity_Ref_Placeholder`, …) and entity references | ✅ (unit) / imported packages set them up automatically | |
| CALS tables: insert dialog, rows, columns, merge, split, Tab | ✅ | |
| Automatic IDs on insert | ✅ | |
| Move elements up/down (keeps whitespace layout) | ✅ | |
| Follow links with Ctrl+click: IDREF, S1000D `dmRef` (opens the other module) | ✅ | Local `href` is ☑️. |
| Drop an image file to insert the package's image element | ✅ | Wrapped in an allowed container (e.g. a figure) when needed. |

## Validation and tools

| Feature | Status | Notes |
|---|---|---|
| Validation as you type; issues list; invalid tags flagged | ✅ | |
| Find and replace; F3 / Shift+F3 | ✅ | |
| Spell check with suggestions and a user dictionary | ✅ | Tested with a small local dictionary. No dictionary ships with Xylem; point Preferences at Hunspell `.aff`/`.dic` files. |
| Format document (pretty-print) | ✅ | Indent amount and per-element "indent content" from the package; mixed content is never touched. |
| Save (lossless, atomic) | ✅ | |
| Autosave and crash recovery | ☑️ | Writes unsaved work every 60 s; offers recovery at the next start. |
| Reopen the previous session | ☑️ | |

## Customisation

| Feature | Status |
|---|---|
| `xylem-import` command line (`.ctm` folders and `.xac` archives) | ✅ (all 137 real sets import and load back) |
| Import from the UI | ☑️ (uses a folder picker that the test harness can't drive) |
| Package matching | ✅ |
| CTM templates, FollowedBy, toggles, lists, images, descriptions, pretty-print, protected PIs | ✅ |
| `.tbr`, `.js`, `.xdp` | ➖ (unused by the legacy host) |

See [docs/doctype-packages.md](docs/doctype-packages.md) for the package format.

## S1000D (issues 2.0–6.0)

| Feature | Status | Notes |
|---|---|---|
| Built-in styled rendering | ✅ | 4.1 descriptive and procedural; legacy markup ☑️. |
| New data module wizard | ✅ | 4.1 descriptive. Other issues/kinds are unit-tested; some skeletons still need checking against the real schemas. |
| BREX check | ✅ | |
| Data module info; rename to the CSDB file name | ✅ | |
| dmRef navigation | ✅ | |
| Validation against the real S1000D schemas | ☑️ | No S1000D schemas are available locally yet. |

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
