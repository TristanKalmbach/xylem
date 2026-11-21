# Doctype packages

A doctype package tells Xylem how to edit one kind of document: which schema
to validate against, how to style it on screen, and how editing should behave
(what Enter does, which element Ctrl+B inserts, what a new list looks like).

A package is a folder with a `doctype.toml` file and the files it refers to:

```
manual/
├── doctype.toml
├── manual.xsd
├── manual.css
└── warning.png        (anything the stylesheet references)
```

Xylem scans its package folders at startup (File › Preferences lists them; by
default `%APPDATA%\Xylem\doctypes` on Windows, `~/Library/Application
Support/Xylem/doctypes` on macOS and `~/.config/Xylem/doctypes` on Linux).
Packages can sit in sub-folders. File › Reload doctype packages picks up
changes without restarting.

`samples/doctypes/manual` in this repository is a complete, small example.

## How a document finds its package

When a document opens, every package's `[match]` rules are checked. A package
qualifies if any of these hit:

| Key | Matches |
|---|---|
| `schema_locations` | Globs against the file name in `xsi:noNamespaceSchemaLocation`, `xsi:schemaLocation` or the DOCTYPE system id |
| `public_ids` | Substrings of the DOCTYPE public id |
| `file_names` | Globs against the document's own file name |
| `roots` | The document element's name |

The highest `priority` wins; ties go to the strongest kind of hit, in the
order of the table above. Documents that match nothing use a built-in generic
package: no schema, a simple default look, and every element allowed.

```toml
[match]
roots = ["manual"]
schema_locations = ["manual.xsd"]
priority = 0
```

## Files

```toml
[files]
schema = "manual.xsd"          # .xsd or .dtd; optional
style = "manual.css"           # the tags-on / normal view stylesheet
structure_style = "manual_structure.css"   # kept for XMAX imports; unused for now
entities = "entities.ent"      # extra entity declarations; optional
```

Paths are relative to the package folder and may not leave it. Without a
`schema`, Xylem uses the schema the document itself points to (resolved
through the schema catalog), and without either it only checks
well-formedness.

## Stylesheet

The stylesheet is ordinary CSS 2.1 applied to the XML elements, plus the
XMetaL extensions that XMAX customizations use:

- `$COMMENT`, `$PROCINS`, `$MARKSEC` and `$DOCUMENT` select comments,
  processing instructions, CDATA sections and the document node.
  `$PROCINS[xm-pi-target="Ref_Label"]` selects one kind of PI.
- `::before` / `::after` with `content:` text, `attr(name)`, `counter(name)`,
  `counters(name, ".")` and `url(image.png)`.
- `display: table | table-row | table-cell` lays out tables; CALS tables also
  pick up column widths from `colspec/@colwidth`.

`samples/doctypes/manual/manual.css` shows most of this in under 40 lines.

## Templates

A template is the markup inserted when the author inserts an element. The
`<?xm-replace_text …?>` processing instruction shows a prompt that disappears
as soon as the author types:

```toml
[templates.warning]
xml = '<warning><para><?xm-replace_text Warning text?></para></warning>'
```

Elements without a template are created from the schema: Xylem adds the
required children and attributes, so the new element is valid straight away.

## Behaviour

```toml
[behaviour]
paragraphs = ["para"]              # Enter splits these

[behaviour.followed_by]            # Enter at the end of <title> starts a <para>
title = "para"

[behaviour.toggles]                # Ctrl+B, Ctrl+I, Ctrl+U and the format menu
bold = "b"
italic = "i"
underline = "u"
superscript = "sup"
subscript = "sub"

[[behaviour.images]]               # what drag-and-drop of an image creates
element = "graphic"
source_attr = "href"

[behaviour.id_generation]          # IDs for newly inserted elements
elements = ["chapter", "section"]
attr = "id"
pattern = "{name}-{seq:3}"         # chapter-001, section-002, …
```

Other keys:

| Key | Effect |
|---|---|
| `read_only_elements` | Content can't be edited |
| `non_removable_elements` | The element can't be deleted (its content can be edited) |
| `protected_pis` | PI targets that can't be deleted on their own (generated labels, placeholders managed by another system) |
| `protect_entity_refs` | Treat `&name;` references the same way |
| `bulleted_lists`, `numbered_lists` | Add toolbar buttons that insert the list with its first item |

## Pretty printing

Format document (Tools menu) re-indents element-only content and never
touches mixed content, so it can't change what the document says.

```toml
[pretty_print]
indent_amount = 2

[pretty_print.elements.para]       # XMAX's per-element layout flags
newline_before_start_tag = true
indent_content = false
```

## Descriptions and toolbar

```toml
[descriptions]
warning = "A safety warning shown in a red box."   # tooltip in the Elements panel

[[toolbar]]
id = "warning"
label = "Warning"
action = "insert-element:warning"   # or toggle:bold, surround:emphasis, save, validate, …
```

Without a `[[toolbar]]` section Xylem builds a toolbar from the toggles,
lists and images in `[behaviour]`.

## Converting XMAX customizations

`xylem-import` turns an XMAX customization folder (with a `.ctm`) or a `.xac`
archive into a package:

```
xylem-import path/to/Customization_Folder out/doctypes/Customization_Folder
xylem-import --all path/to/root out/doctypes
```

It converts the CTM's templates, FollowedBy rules, lists, toggles, images,
descriptions and pretty-print settings; copies the schema, stylesheets and
their images; and marks PIs that the stylesheets style through `$PROCINS` as
protected. The report lists anything it skipped. `.tbr` toolbars and `.js`
macros are not converted.
