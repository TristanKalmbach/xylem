# Tracked changes and comments

Xylem records review changes the way XMetaL does: as processing
instructions inside the document. A reviewed file is still ordinary XML. It
validates as before, because processing instructions sit outside the content
model, and it can go back and forth between Xylem and XMetaL users.

## Using it

| Action | Where |
|---|---|
| Turn tracking on or off for the current document | Review › Track changes, Ctrl+Shift+T |
| Show the list of changes and comments | Review › Review panel |
| Add a comment at the caret, or on the selected text | Review › New comment…, Ctrl+Shift+M (Ctrl+Enter saves it) |
| Accept or reject the change at the caret | Review › Accept / Reject change at caret |
| Accept or reject every change | Review › Accept all / Reject all, Ctrl+Shift+A / Ctrl+Shift+R, or the buttons in the panel |
| Edit or delete a comment | The Edit and Delete buttons in the Review panel, or double-click the comment marker |

The author's name comes from File › Preferences › Review author. When that
is empty, Xylem uses your login name. Tracking is turned on per document and
is not remembered when the document is closed. Comments can be added whether
tracking is on or off.

In the document view, tags on or off:

- inserted text is underlined, in a colour that belongs to its author;
- deleted text is struck through in the same colour, and can't be edited;
- a comment shows the author's initials on a yellow background, and text
  that a comment covers is highlighted;
- hovering over any of these shows who made the change, when, and what it
  says.

The Review panel lists every change and comment in document order. Click an
entry to move the caret there.

### What is tracked

When tracking is on:

- Typing and pasting record an insertion. Typing next to, or inside, an
  insertion of your own extends it rather than starting a new one.
- Backspace and Delete on text that was already there record a deletion.
  Further deletions next to it are added to the same mark. Deleting text that
  you inserted yourself just removes it.
- Replacing a selection records a deletion followed by an insertion. Find and
  replace and spelling corrections are tracked the same way.
- When a deleted range covers inline markup, such as a `<b>` inside the
  deleted words, the markup goes into the deletion. Rejecting the deletion
  restores it exactly. When the range crosses paragraphs, the paragraphs stay
  and only their text is marked deleted.
- Typing inside someone else's insertion splits it: theirs ends, yours
  follows, and theirs resumes after it.

Structural edits are not tracked in this version. These happen as ordinary
edits even with tracking on:

- splitting and joining paragraphs (Enter, and Backspace at the start of a
  paragraph);
- inserting, removing, renaming and moving elements;
- attribute changes and table operations;
- pasting whole blocks between paragraphs.

Accepting or rejecting a change is one undo step, and so is Accept all or
Reject all.

## The markup

```xml
<?xm-insertion_mark_start author="Ann" time="20261121T101500+0000"?>new text<?xm-insertion_mark_end?>
<?xm-deletion_mark author="Ann" time="20261121T101500+0000" data="old &amp;lt;b&amp;gt;words&amp;lt;/b&amp;gt;"?>
<?xm-annotation author="Ann" time="20261121T101500+0000" data="Check this value"?>
<?xm-annotation_start author="Ann" time="20261121T101500+0000" data="Check this"?>commented text<?xm-annotation_end?>
```

- `author` is the reviewer's name.
- `time` is `YYYYMMDDThhmmss` followed by the UTC offset as `±hhmm`.
- The `data` of a deletion mark is the deleted content as an XML fragment.
  The fragment is escaped once more to fit in the attribute, which is why
  `<b>` appears above as `&amp;lt;b&amp;gt;`. Keeping markup there is what
  lets a rejected deletion bring back an element.
- The `data` of a comment is the comment text.
- An insertion or a commented range is everything between its start and end
  marks, in document order. Insertions do not nest.

### Assumptions

No file written by XMetaL was available when this was built, so the format
follows the names XMetaL documents for these instructions, not files it has
been seen to write. In particular, these points have not been checked against
real XMetaL output:

- that `author`, `time` and `data` are the attribute names, and that the time
  has exactly this form;
- that deleted markup in `data` is escaped as described above;
- that comments use `xm-annotation` (or the `_start`/`_end` pair) with the
  text in `data`.

Reading is lenient to allow for differences: attributes may come in any
order and use either quote style. Attributes Xylem doesn't know are kept
when it rewrites a mark (to extend a deletion, or edit a comment). If XMetaL
turns out to use other names, `crates/xylem-edit/src/revision.rs` is the one
place to change them.

Times are written in UTC (`+0000`). The Rust standard library cannot tell
the local time zone, and UTC keeps the stamps exact. They are shown in the
Review panel as stored.

Marks you don't touch are saved byte for byte, like everything else in the
document.
