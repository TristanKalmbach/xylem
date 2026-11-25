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

**Platforms.** Built and tested on Windows 11 and on Linux (Ubuntu 24.04
under WSL, X11). CI for Windows, macOS and Linux is in
`.github/workflows/ci.yml`; it has not run yet, so macOS is unverified.

**Where replacement stops.** Xylem replaces XMAX as the editor people author
in. It is a separate program, not a control the legacy host can embed, so a
host that drives XMAX through its COM interface (opening documents from its
own storage, its ribbon commands, check-in) keeps doing so until that host is
changed to hand documents to Xylem. Nothing in Xylem depends on such a
change: documents, schemas, CSS and imported customizations work as they
are. For XSLT, the built-in engine is offered to such hosts as a C library
with the calling convention of their current engine; see
[docs/embedding.md](docs/embedding.md).

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
| Chinese, Japanese and Korean text | verified | Fonts for these, Thai, Indic and symbols load when a document needs them (Windows fonts, or Noto on Linux). In a file mixing Japanese and Chinese, Chinese-only characters sit slightly lower on the line. |
| Arabic and Hebrew text | verified | In the tags-on and normal views Arabic letters take their joined forms (with the lam-alef ligature) and text is laid out by the Unicode bidirectional algorithm: right-to-left paragraphs start at the right, wrapped lines stay in reading order, numbers and Latin words read left to right inside them, brackets are mirrored and tag chips point the way the text reads. Clicking, the caret, selection, double-click, Home/End and the arrow keys follow the text. Checked against the same text rendered by Edge, and by scripted clicks and typing (`scripts/e2e-smoke.sh`). Structure tree previews and the HTML export read correctly too. Limits: the Source view and editable fields (attribute values, find box) show the letters unjoined, left to right; marks are placed where the font puts them, and ligatures other than lam-alef are not formed. |
| Thai and Indic text | partial | The characters show, but marks and conjuncts are not shaped; the editor says so when such a document opens. The interface toolkit (egui) has no text shaping. |
| Screen readers | tested | Every control, menu item, tree row and tab has a spoken name and state (checked through Windows UI Automation, which Narrator, NVDA and JAWS use); the document area reads as a text field with the current paragraph. Not yet tried with an actual screen reader. |
| Zoom, light and dark themes | verified | Both kept across a restart. |

## CSS

| Feature | Status |
|---|---|
| All 90 customer stylesheets parse with no warnings | verified (corpus test) |
| Selectors, generated content, counters, XMetaL `$PROCINS`/`$COMMENT`, `@import` | verified |
| Display types, fonts, colours, boxes, alignment, `white-space`, sup/sub, list markers | verified |
| `wrap-option: emergency` | partial (parsed; long words already wrap) |
| Runtime inline style overrides (`ContainerStyle`) | n/a (the host used it only to style entity references, which Xylem styles itself) |

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

## Review (tracked changes and comments)

Stored as XMetaL's `xm-insertion_mark`, `xm-deletion_mark` and
`xm-annotation` processing instructions; see
[docs/track-changes.md](docs/track-changes.md). The names and attributes
come from a description of XMetaL's markup and were not checked against
XMetaL itself.

| Feature | Status | Notes |
|---|---|---|
| Insertions underlined, deletions struck through, per-author colour | verified | Tags-on and normal views. |
| Comment markers and highlighted commented text | verified | |
| Author, time and text on hover | verified | In the tags-on view; normal view uses the same code. |
| Review panel: list in order, click to go there | verified | |
| Track changes toggle (Review menu, Ctrl+Shift+T) | verified | Per document, not remembered. |
| Typing extends one insertion; Backspace records a deletion | verified | |
| Selection replace records deletion then insertion | verified | |
| Find and replace (including Replace all) tracked | verified | |
| Delete key, paste and spelling fixes tracked | tested | |
| Enter inside your own insertion | tested | The paragraph break becomes part of the insertion; Reject all joins the paragraphs again. |
| Select all and Backspace with tracking on | verified | Rows, list items and colspecs stay; Reject all restores the file byte for byte. |
| Deleting your own inserted text just removes it | tested | |
| Deleted inline markup restored on reject | tested | |
| Accept / reject the change at the caret | verified | |
| Accept all / reject all, each one undo step | verified | |
| New comment at the caret or on a selection | verified | |
| Edit and delete comments in the Review panel | verified | |
| Review author in Preferences (login name by default) | verified | Set in Preferences and seen in the saved marks. |
| Untouched marks saved byte for byte | verified | |
| Inserting and deleting elements tracked | verified | An inserted paragraph shows as an insertion and a deleted list item as a deletion; Reject all gave back the original file byte for byte. Empty elements that were in the file are recorded too. |
| Splitting, joining, renaming and moving elements tracked | verified | In the app, Reject all gave back the sample byte for byte after a split, a join, a table-row move and a rename. Unit tests for each. Edits that the marks can't record without loss are refused with a message (see docs/track-changes.md). Surround, remove tags, attributes and other table edits stay untracked. |
| Time stamps in local time | verified | Local time with the UTC offset (`-0500`). |

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
| Autosave and crash recovery | verified | Killed mid-edit and relaunched: the work was offered back, stayed unsaved until saved, and the copy was removed after saving. "Later" keeps the copies for the next start. |
| Changed on disk by another program | verified | A document without unsaved changes reloads by itself; one with unsaved changes asks (keep mine, reload, or open the disk version read-only). Save checks again before overwriting. |
| Two people opening the same file | verified | A hidden `.~lock.<name>#` file next to the document names who has it open; others are offered a read-only copy. |
| Previous versions kept on save | verified | The last ten versions per document, in the config folder; File › Previous versions opens one read-only. |
| Log file and crash reports | verified | `logs/xylem.log` in the config folder (rotated); a panic writes a crash report there and the next start says so. Help › Open log folder. |
| Byte-for-byte round trip on real files | tested | Every well-formed XML file in a corpus folder (`XYLEM_XML_CORPUS`) saves back unchanged, and a one-text edit leaves the rest alone. Run on 830 files (configuration, translations, generated reference XML, UI layouts) and on 247 files from real authored publication exports (2026-11-25: all 247 exact). |
| Random editing keeps documents well-formed, undo is exact | tested | Seeded random edits (typing, deleting, Enter, joining, replacing selections, deleting, renaming and moving elements): 100,000 runs of 60 edits in each of four modes (tracking off; one author; two authors; tracking switched on and off). After every edit the document is well-formed and insertion marks pair up; at the end undo and redo are exact, Reject all gives back the original (when every edit was tracked), and resolving changes one at a time in random order leaves none. |
| Tracked edits on real files resolve exactly | tested | `corpus_edits` makes random tracked edits on every file in `XYLEM_XML_CORPUS` and checks that Reject all and Undo all give back the file. 705 files, 27,600 edits: no content changed. On 164 real authored publication exports, 5,348 tracked edits: all resolved exactly, none re-escaped. Text that was edited and then rejected can come back with characters written differently (`&#174;` as `®`); the content is the same. |
| Reopen the previous session | verified | |
| Run a stylesheet on the document (Tools › Run stylesheet) | verified | Built-in engine; the result opens in Xylem or the browser and is labelled as the UTF-8 it is. Scripted check in `scripts/e2e-smoke.sh`. |
| Publication preview (one or two stylesheets, from Preferences or the team configuration) | verified | One step: the output is named for what it is, so an HTML page opens in the browser. Two steps (XSL-FO, then FO to HTML) checked on real publication stylesheets. Parameters applied. Scripted check in `scripts/e2e-smoke.sh`. |

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
| New publication module (wizard) | verified | Issues 4.0 to 6.0 validate against the official `pm.xsd` (`tests/real_schemas.rs`); created and validated in the app as 4.1. 2.x/3.0 publication modules are not offered. |
| Project view: module list of a folder, text filter, double-click to open | verified | Scans sub-folders on a worker thread (4,000 small modules in about 0.1 s in a release build); the folder is remembered. |
| Check references: unresolved dmRef/pmRef, references to an older issue, superseded modules | verified | Clicking a result opens the module with the reference selected. References to the BREX are counted apart. |
| Insert reference (dmRef, pmRef) from the project view | verified | Complete reference with issue, language and title, placed where the schema allows it; the publication module stays valid. 4.0+ markup only. |
| Applicability filter, issues 4.0 to 6.0 (dim or hide) | verified | `assert`/`evaluate`, value lists and ranges, `applicRefId`, module-level applicability. Values are picked from those the asserts mention or typed. |
| Applicability filter, issue 3.0 | tested | Same model with the 3.0 names (`actidref`, `refapplic`, `inlineapplics`). |
| Applicability filter, issues 2.0 to 2.3 | tested | Model, version and serial-number ranges from `applic/model`, with content pointing at versions through `refapplic`. Not defined by the specification; see docs/s1000d.md. |

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
