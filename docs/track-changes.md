# Tracked changes and comments

Xylem records review changes in XMetaL's style, as processing
instructions inside the document. A reviewed file is still ordinary XML. It
validates as before, because processing instructions sit outside the content
model. The markup is meant to match XMetaL's so files can go back and forth
between Xylem and XMetaL users, but that has not been checked yet; see
"Assumptions" below.

## Using it

| Action | Where |
|---|---|
| Turn tracking on or off for the current document | Review › Track changes, Ctrl+Shift+T |
| Show the list of changes and comments | Review › Review panel |
| Add a comment at the caret, or on the selected text | Review › New comment…, Ctrl+Shift+M (Ctrl+Enter saves it) |
| Accept or reject the change at the caret | Review › Accept / Reject change at caret |
| Accept or reject every change | Review › Accept all / Reject all, Ctrl+Shift+A / Ctrl+Shift+R, or the buttons in the panel |
| Edit or delete a comment | The Edit and Delete buttons in the Review panel, or double-click the comment marker |

The author's name comes from File › Preferences › General › Author. When that
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
  and only their text is marked deleted. Where text isn't allowed (between
  list items, table rows or column specifications) nothing is deleted, so
  selecting everything and pressing Backspace marks all the text deleted
  and leaves the structure as it was.
- Pressing Enter inside your own insertion makes the new paragraph break
  part of it.
- Typing inside someone else's insertion splits it: theirs ends, yours
  follows, and theirs resumes after it.

Inserting and deleting whole elements is tracked too. A new element (from
the Elements panel, Ctrl+Space, a table or a template) gets insertion marks
around it, so rejecting it removes it. Deleting an element replaces it with
a deletion mark that holds its markup, so rejecting puts it back exactly,
including any changes it held. Deleting an element you inserted yourself, or
the empty paragraph left behind when you split one at its end, simply removes
it. An empty element that was in the file (`<companyName/>`, say) is recorded
like any other.

Changes to the structure are tracked with the same marks:

- **Splitting** a paragraph (Enter) records the break as an insertion that
  starts at the end of the first part and ends at the start of the second.
  Rejecting it joins the two again; Backspace at the start of the second
  part takes your own break out.
- **Joining** two paragraphs (Backspace or Delete at the boundary) keeps the
  second paragraph in a deletion mark and marks the text that moved into the
  first as inserted. Rejecting both gives back the two paragraphs exactly.
- **Renaming** an element keeps the old one in a deletion mark in front of
  it and marks the renamed one inserted.
- **Moving** an element up or down leaves a deletion mark where it was and
  marks it inserted where it went. Moving an element you inserted just
  moves it.
- Typing over an `xm-replace_text` placeholder keeps the placeholder in a
  deletion mark, so rejecting brings it back.

Some edits are refused while tracking, because the markup can't record them
without losing something. Xylem says why and asks you to accept or reject
the changes involved first:

- renaming or moving an element that holds tracked insertions (insertions
  can't nest);
- joining a paragraph that holds another author's pending insertions (they
  would be folded into yours);
- joining a paragraph that is one half of a tracked split;
- joining two paragraphs across a tracked deletion that lies inside an
  insertion.

Deleting one half of a tracked split is allowed: its content is marked
deleted and the element stays, so the split can still be rejected. With
tracking off, the halves of a tracked split can't be moved past each other.

Anything you add between the two halves of someone else's pending split
(a paragraph inserted there, say) becomes part of their insertion, so that
rejecting their split still joins the halves.

Join, rename and move each show as two changes in the Review panel, a
deletion and an insertion. Accept or reject both together: accepting one
and rejecting the other leaves the text twice or not at all.

The split, join, rename and move encodings (an insertion that runs across
an element boundary, whole elements inside a deletion mark) are Xylem's own
use of the XMetaL marks and have not been checked against XMetaL itself.

Renaming the root element, surrounding and removing tags, attribute
changes, table operations other than moving rows, and pasting whole blocks
between paragraphs are still ordinary edits with tracking on.

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

Neither a file written by XMetaL nor XMetaL's documentation was available
when this was built. The instruction names and attributes come from a
written description of XMetaL's markup, so none of it has been confirmed
against what XMetaL really writes. In particular:

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

Times are written in the author's local time with its UTC offset
(`20261121T145816-0500`), as XMetaL does.

Marks you don't touch are saved byte for byte, like everything else in the
document.
