# UI design notes

How the editor's own interface is put together, for anyone changing it. The
look of a *document* is not covered here: that comes from its doctype
package's stylesheet.

## Theme

`crates/xylem-app/src/ui/theme.rs` defines a `Palette` for each theme
(chrome, panel, surface, raised, text in three strengths, one accent, and
success/warning/danger with soft backgrounds) plus spacing, radius and type
scales. Read the palette with `ui::palette(ctx)` rather than picking colours
in place. The exceptions are deliberate: syntax colours in the source view,
per-author colours for tracked changes, the darker red of the danger button
and the document's own CSS colours.

A unit test checks text contrast for every pair the UI uses, in both themes
(7:1 for body text, 4.5:1 for secondary and status text). Tag chip colours
come from a fixed set of hues and have their own contrast test.

The theme follows the system until the user picks one in Preferences or the
View menu. `XYLEM_THEME=light|dark` overrides both, for screenshots.

## Components

`crates/xylem-app/src/ui/widgets.rs` holds the shared pieces: icon buttons,
primary, secondary and danger buttons, segmented controls, panel tabs,
section headers, list rows, badges, notices, cards, two-column forms and the
dialog shell. Prefer adding a component there to styling a widget in place.

Dialogs (`Dialog`) have a title, a close button and a footer from
`dialog_footer`. The primary action is rightmost; Cancel sits to its left.
Enter confirms (except inside a multi-line field) and Escape closes. Modal
dialogs dim the window; tool windows such as Find stay modeless.

Icons are Phosphor (MIT), embedded through the `egui-phosphor` crate.

## Layout

Menu bar and toolbar on top, document tabs over the document, side panels
left (Structure, Project) and right (Elements, Attributes, Review), issues
below the document, status bar at the bottom. In windows narrower than about
1000 points the right panels give way first, then the left one under 760.

## Testing the UI

Scripted runs (`XYLEM_SCRIPT`) find widgets by label, text or command name,
so they keep working when the layout changes. Every shared component
registers its label for `clickui`. `XYLEM_WINDOW_POS` and `XYLEM_WINDOW_SIZE`
place and size test windows; scripted windows never take focus, have no
taskbar button and run at idle priority.
