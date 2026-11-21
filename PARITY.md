# XMAX parity checklist

This checklist is built from a survey of how our existing authoring host uses XMetaL XMAX 20. The survey covered the host application and the per-customer XMAX customizations (`.ctm`, `.css`, `.xsd`, `.tbr`, `.js`).

**Scope.** Parity means *authoring* features. Xylem is a standalone editor, not an ActiveX/COM control. That puts the XMAX COM object model out of scope: events such as `OnUpdateUI`, `OnContextMenu` and `OnMessage`, and properties such as `ChangedNodes` and `FormattingUpdating`. Where the host used one of those APIs to build a feature users see, the feature is listed instead.

**Legend**
- ✅ Verified in the running app, through the scripted end-to-end harness and screenshots.
- ☑️ Implemented and unit-tested, but not yet exercised in the running app.
- 🟡 Partial.
- ⬜ Not done.
- ➖ Not applicable.

**Platform.** Built and tested on Windows 11 only. Every dependency is pure Rust and cross-platform: eframe/egui, and rfd with the XDG portal on Linux. macOS and Linux builds still need to be tried.

## Document views

| Feature | Status | Notes |
|---|---|---|
| Tags-on view driven by CSS | ✅ | Checked with the sample doctype, the built-in S1000D package, and a real 33k-element customer manual opened with its imported XMAX customization. |
| Normal view (no tags) | ✅ | |
| Source (plain-text) view | ✅ | Switching to source and back round-trips byte for byte. Well-formedness errors show line and column. |
| Structure tree | ✅ | Click selects an element. It follows the caret. Its context menu (remove, delete, move) is ☑️. |
| Graphical tables | ✅ | CALS and HTML, with `colwidth` proportions and horizontal spans. Vertical spans (`morerows`) are kept in the data but not drawn merged. |
| Inline images | ✅ | Missing files show as labelled boxes. Drawing real PNG and JPEG files is ☑️, since no sample images exist. CGM and SVG show a placeholder. |
| Comments shown | ✅ | |
| Collapsed container tags | ⬜ | Collapsing works in the structure tree, not in the canvas. |
| Zoom; light and dark themes | ☑️ | |

## CSS (the full set used by the 90 customer stylesheets)

| Feature | Status |
|---|---|
| All 90 customer stylesheets parse with 0 warnings or unsupported constructs | ✅ (corpus test) |
| Selectors, `:before`/`:after`, `attr()`, counters, XMetaL `$COMMENT` and `$PROCINS` selectors, `@import` | ✅ |
| Display types, fonts, colours, boxes, alignment, `white-space`, sup/sub, list markers | ✅ |
| `wrap-option: emergency` | 🟡 (parsed; egui breaks long words anyway) |
| `url()` images in generated content | 🟡 (parsed, not drawn) |
| Runtime inline style overrides (`ContainerStyle`) | 🟡 (supported by the engine; no UI) |

## Editing

| Feature | Status | Notes |
|---|---|---|
| Typing; mouse and keyboard caret and selection | ✅ | Done on the sample and on a scratch copy of the real customer manual. |
| Enter splits the element, following CTM `FollowedBy` | ✅ | |
| Backspace/Delete that respect markup; deleting a selection | ✅ | |
| Bold, italic and other toggles, using the package's mapping | ✅ | The real customer package mapped Ctrl+B to `<emphasis>`. |
| Schema-aware insert from the Elements panel and context menu | ✅ | Templates come from the CTM; otherwise a required skeleton is generated. |
| `xm-replace_text` placeholder prompts replaced by typing | ✅ (unit test) / shown ✅ | |
| Surround, remove tags, split, join, change element | ☑️ | |
| Paste plain text, and XML fragments with schema checking | ✅ | A fragment that isn't allowed falls back to text, as XMAX's `PasteStringWithInterpret` did. |
| Copy and cut XML fragments | ☑️ | |
| Undo and redo (journaled, no document copies) | ✅ | Typing is grouped into one step. The dirty marker is exact across undo, redo and save. |
| Insert comment, PI or CDATA; double-click to edit | ☑️ | |
| Read-only and non-removable elements (per package) | ☑️ | |
| Read-only `Ref_Label`, `Entity_Ref_Placeholder` and `cms-replace_content` PIs | ⬜ | The legacy host protected these. Here they can be deleted like any other PI. |
| CALS tables: insert dialog, add or remove rows and columns, merge, split, Tab | ✅ | Insert, add column right, merge down and Tab were done in the app. The rest are unit-tested. |
| Auto IDs on insert (package `id_generation`) | ☑️ | |
| Follow link with Ctrl+click (IDREF, local href, S1000D dmRef) | ✅ (IDREF) / ☑️ (href, dmRef) | |
| Dropping image files to insert a graphic | ⬜ | Dropping files opens them as documents. |

## Validation and tools

| Feature | Status | Notes |
|---|---|---|
| Schema validation as you type; issues list; invalid tags flagged | ✅ | The real customer manual validates with 0 errors. The corpus run found 35 real data errors across 52 instances. |
| Find and replace (case, whole word, regex, limit to an element, replace all) | ✅ | Replace-all through the dialog is one undo step. |
| Spell check (Hunspell via `spellbook`) | ☑️ | No dictionary is bundled or installed here. Set a dictionary folder in Preferences. |
| Pretty print | 🟡 | A generic pretty-printer runs in Source view. The CTM's per-element options are imported but not applied. |
| Save | ✅ | Untouched markup is written byte for byte, and the save is atomic (write to a temp file, then rename). |

## Customisation (XMAX → Xylem doctype packages)

| Feature | Status |
|---|---|
| `xylem-import` command line: `.ctm` folders and `.xac` archives | ✅ (all 137 real sets import and load back) |
| Import from the UI (File › Import XMAX customization) | ☑️ (needs a folder-picker dialog that the test harness can't drive) |
| Package matching (schema location, root, public ID, file name, priority) | ✅ |
| CTM templates, FollowedBy, toggles, lists, images, descriptions | ✅ |
| `.tbr`, `.js`, `.xdp` | ➖ (the legacy host used none of them) |

## S1000D (issues 2.0–6.0)

| Feature | Status | Notes |
|---|---|---|
| Built-in styled rendering | ✅ | 4.1 descriptive and procedural. Legacy 3.0 markup is ☑️. |
| New data module wizard | ✅ | 4.1 descriptive. Other issues and schema types are unit-tested; some skeletons were written from memory and need checking against the real schemas. |
| BREX check | ✅ | Run against a sample project BREX. SNS and notation rules are not enforced. |
| Data module info; renaming the file to its CSDB name | ☑️ | |
| Validation against real S1000D schemas | ☑️ | No S1000D schemas are available locally. The DTD and XSD engines are tested with synthetic S1000D-like grammars. Point Preferences at a schema folder or catalog. |

## Performance (release build, Windows, measured)

| Document | Typing (per key) | Structural edit (Enter, insert) |
|---|---|---|
| Sample manual (≈100 elements) | < 2 ms | < 5 ms |
| Synthetic manual (20k elements, 1 MB) | ≈ 30 ms | ≈ 40 ms |
| Real customer manual (33k elements, 2.4 MB) | ≈ 70 ms | ≈ 230 ms |

Undo memory grows with the size of each edit, not the size of the document. The next step for speed is incremental layout, which caches the laid-out paragraphs of unchanged subtrees.
