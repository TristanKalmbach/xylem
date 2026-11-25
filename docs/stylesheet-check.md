# Checking stylesheet output

Tools › Check stylesheet output runs an XSLT stylesheet on a set of test
inputs and compares the output with a baseline, so a stylesheet can be
changed (made faster, tidied up) with proof that what it produces stays the
same. The same check runs from a terminal with `xylem-xslt-check`.

## What it compares with

- **Last committed version**: the same stylesheet as last committed to git.
  Use this while editing: it shows whether your uncommitted changes alter
  the output. Xylem copies the committed stylesheet files of the folder tree
  the stylesheet lives in (stylesheets and the XML, entity and DTD files they
  read) once per commit, and reuses the copy.
- **Saved output**: output saved for each test, for example by the
  publishing service that normally runs the stylesheet.
- **Another stylesheet**: any other stylesheet file.

Output is compared after setting aside differences that are not real: a byte
order mark, and the document number inside IDs made by `generate-id()`
(Saxon numbers documents with a counter that keeps rising while it runs, so
`d31e20` in one run is `d7709e20` in another). Everything else counts. Each
difference is listed with the element it is in, the nearest `id` around it,
and the text before and after.

**Open report** writes the results as a page and opens it in the browser:
every test with its result and time, and for each test that changed, every
place it changed, with the unchanged text around it and what was removed and
added highlighted. It is a single file, so it can be saved or sent with the
change. From a terminal: `xylem-xslt-check suite.toml --report check.html`.

## The engine

Stylesheets run on Xylem's own XSLT 2.0 engine, which needs no .NET, Java or
Saxon. It runs stylesheets the way the reference publishing service does, so
its output can be compared with what that service saved: XSLT 2.0
stylesheets (root `version="2.0"`) with the service's XSLT 2.0 rules, and
any other version with its XSLT 1.0 rules, including how the service writes
the HTML of the FO-to-HTML step. See [design/xslt-engine.md](design/xslt-engine.md)
for what it supports and [design/conformance.md](design/conformance.md) for
its W3C test-suite results.

### The .NET host (optional reference)

`tools/saxon-dotnet` builds `SaxonDotnetHost.exe`, a program that runs
stylesheets on Saxon-HE and .NET's XSLT 1.0 processor exactly as a .NET
publishing service does (see its README; you need your own copy of the Saxon
assemblies). Xylem no longer needs it. Use it as a reference:

- **Compare with** (Preferences › Stylesheets, `xslt_reference` in the team
  configuration, or `--reference <program>`): Check also runs the stylesheet
  on the host and flags every test whose output differs between the two
  engines; the exit code is non-zero if any does.
- **Engine** (Preferences › Stylesheets or the Check window's Engine field,
  `xslt_host`, or `--host <program>`): run everything on the host instead of
  the built-in engine.

Embedded scripts in XSLT 1.0 stylesheets (`msxsl:script`) run only on the
host, and only when the team configuration sets `xslt_allow_scripts = true`
or `--allow-scripts` is given on the command line.

### Comparing two engines on every stylesheet

Before replacing one engine with another, run every stylesheet you have
through both and compare:

```
xylem-xslt-check differential <stylesheets folder> --inputs <folder of inputs> \
    --input-pattern "*.xml" --reference tools/saxon-dotnet/bin/SaxonDotnetHost.exe \
    --params "mode=final" --params "mode=draft" [--threads 4] [--work <folder>]
```

Every top-level stylesheet under the folder (one no other file includes or
imports) runs on every input, once per `--params` set, on the reference and
on the built-in engine, and the outputs are compared as Check compares them.
Engines that both fail on a case agree. Outputs that agree are deleted as
they are compared; ones that differ are kept in the work folder. Results go
to `results.jsonl` there as each stylesheet finishes, so a run that is
stopped and started again with the same `--work` carries on where it was,
and `report.md` lists every disagreement. Add `--serialize` to compare a
second step such as XSL-FO to HTML (inputs read as they are, results
written as `xsl:output` asks). The exit code is 0 only when the engines
agree everywhere.

Generated IDs (`generate-id()`) differ between engines without changing
what refers to what: Saxon's `d12e345` names a document and a node, and
engines number temporary documents differently (one builds a tree once per
loop, another per iteration); .NET's XSLT processor writes IDs such as
`ID0EVEAC`. `--rename-ids` treats two outputs as the same when one's
generated IDs can be renamed one to one into the other's, so every element
must still carry the ID that its references use, and no two elements may
share one where the reference engine gives them different IDs. Check itself
always compares IDs in full (with document numbers renumbered in order of
appearance).

`xylem-xslt-check run …` and `compile …` take the same arguments as
`SaxonDotnetHost.exe` and answer with the built-in engine, so the check
program can be used wherever an engine program is expected, to run the
built-in engine in a process of its own.

## Test suites

A suite is a TOML file listing the test inputs. Paths are relative to it.

```toml
stylesheet = "../stylesheets/book-to-fo.xsl"

[params]            # passed to every test
mode = "draft"
draft = "yes"

[[test]]
name = "Large manual"
input = "large-manual.xml"
expected = "large-manual.fo"   # saved output, for "Saved output"
params = { mode = "final" }     # overrides for this test
```

Pick inputs that exercise the stylesheet well: a large publication, and one
of each kind the stylesheet handles. Keep suites with real content out of
public repositories.

### Making a suite from saved publications

A publishing system often keeps each publication's input and the output it
made side by side. **New…** next to the suite field makes a suite from such a
folder (and its subfolders): give a pattern for the inputs and one for the
saved outputs, with `*` standing for the part they share. For inputs named
`manual-input.xml` with outputs named `manual.fo`, that is `*-input.xml` and
`*.fo`. Inputs without a matching output are listed without one. The suite
is written as `xylem-tests.toml` in that folder, using the stylesheet in the
window; add the stylesheet's parameters to it by hand.

From a terminal:

```
xylem-xslt-check new-suite <folder> --input "*-input.xml" --expected "*.fo"     --stylesheet stylesheets/book-to-fo.xsl --param mode=final [--out suite.toml]
```

## From a terminal

```
xylem-xslt-check suite.toml [--stylesheet file.xsl]
                 [--against saved | committed[:<rev>] | other.xsl]
                 [--host SaxonDotnetHost.exe | builtin] [--work folder]
```

`--host` defaults to the `XYLEM_XSLT_HOST` environment variable, and to
Xylem's built-in engine when that is not set either. The exit
code is 0 when every test gave the same output, so the check can run in a
build.

## Finding the slow templates

**Time templates** in the same window runs the stylesheet on every test with
timing switched on and lists its templates and functions by the time spent in
each: its own time (excluding the templates it called), its total time
(including them), how often it ran, and its share of the whole. Click one to
open the stylesheet at that template.

Timing slows the run down, so use the numbers to compare templates with each
other, not to predict publishing time. A template with a large own time is
where an optimisation pays off; check the change with **Run check**
afterwards.

From a terminal: `xylem-xslt-check suite.toml --profile [--top 25]`.

On the .NET host only XSLT 2.0 stylesheets can be timed; the built-in engine
times both.

## Finding and fixing slow patterns

**Analyze** times the stylesheet on the tests, then reads it and everything
it includes or imports, looking for patterns that make stylesheets slow.
Each finding shows the template or function it is in, how much time that
took and how often it ran, so the ones that matter come first. Click the
place to open it.

There are three kinds of finding:

- **Fix**: a rewrite that gives the same result by construction and stays
  inside the element it came from. Today: a lookup in an `xsl:when` test
  that the code inside it evaluates again is computed once, in a variable
  just before the `xsl:choose`.
- **Suggestion**: a rewrite that is equivalent only under conditions the
  analyser cannot see, or that adds a top-level declaration. Today: a
  whole-document search for a value (`//figure[@id = $ref]`) becomes an
  `xsl:key` lookup. **Apply and check** applies one on request.
- **Note**: a costly pattern with no mechanical fix, with what to do
  instead: `xsl:number level="any"`, `preceding::` and `following::`,
  searches with `//` or `descendant::` inside templates, and searches by
  value through a list held in a variable.

**Apply N fixes** applies the fixes in code the tests reach (a fix the tests
do not reach cannot be proven, so it is listed but not applied), then
proves them:

- every test suite in the suite's folder whose stylesheet uses a changed
  file is checked against the chosen baseline (normally the last committed
  version), and
- every top-level stylesheet in the same tree (the top folder of the git
  repository the stylesheet is in) that uses a changed file is compiled.

A fix in a shared module (a base stylesheet that many import) reaches
stylesheets no suite covers; compiling them at least shows they still load.
**Undo** puts the files back as they were. Fixes change only the characters
of the expressions they rewrite and the lines they add, so the rest of each
file stays byte for byte as it was. A file open in Xylem with unsaved
changes is not touched.

The fixes are XPath 2.0. On a stylesheet that runs on an XSLT 1.0 processor
Analyze still lists what it finds, but as notes with nothing to apply.

From a terminal:

```
xylem-xslt-check suite.toml --analyze [--top 40]
xylem-xslt-check suite.toml --analyze --fix --scope <stylesheets folder> [--repeat 5]
```

`--repeat 5` times each test five times and reports the median, for
comparing speed before and after.

## Errors as you type

While a stylesheet is open, Xylem compiles it in the background whenever
typing pauses, together with every top-level stylesheet that includes or
imports it (so a module is checked in the context it is used in), using the
text in the editor for any stylesheet with unsaved changes. Errors appear in
the Issues panel with their file and line: those in the open file mark the
element they are on, and clicking any of them opens its file at that line,
in the structured or the Source view.

## Running a stylesheet on a document

**Tools › Run stylesheet** (Ctrl+Shift+R) runs a stylesheet on a document
and opens the result: in Xylem, or in the browser when it is an HTML page.
The stylesheet and document are filled in from the open documents (the
active one is the stylesheet or the document, whichever it is), with the
parameters as `name=value` pairs. A document open in Xylem is read as it is
in the editor, unsaved edits included; the stylesheet runs as saved, and
the window says so when it has unsaved changes. **Output** chooses between
the way a publishing service saves a result (as Check compares it) and the
way the stylesheet's `xsl:output` asks. Results go to `xylem-run` in the
temporary folder, named after the document, and a result already open in
Xylem is reloaded. The file is UTF-8 and says so: a publishing service's
second step writes to a string, so its XML declaration or HTML content-type
tag names `utf-16`, and that label is corrected in the saved file (Check and
the differential keep the service's bytes). `samples/xslt` has a small
stylesheet to try this on `samples/manual-sample.xml`.

The F12 references window opens at the right edge of the window, so the
place it takes you to stays in view.

## Finding your way around a stylesheet

**Tools › Stylesheet references** (F12), in a stylesheet, lists what the
element at the caret defines or uses: named templates, functions, keys,
modes and variables. For each it shows where it is defined and every place it
is used, in this file and in every stylesheet that includes or imports it
(found in the same folder tree: the top folder of the git repository, or the
file's own folder). Click one to go there. With the caret on an element that
names nothing, the nearest enclosing element that does is used, so the caret
can be anywhere inside a template.

A name defined more than once (a template overridden in another module, a
function redefined for one kind of publication) lists every definition.
Functions are matched by namespace, not prefix. Variables follow XSLT's
scoping: a reference goes to the nearest declaration before it, else to the
global one. Stylesheets with unsaved changes are read as they are in the
editor.

## Publication preview for authors

**File › Publication preview** (Ctrl+Shift+F5) runs the publishing
stylesheets on the document as it is in the editor, unsaved edits included,
and opens the result in the browser. With a stylesheet that turns XSL-FO into
HTML (such as a publishing system's own FO-to-HTML stylesheet) the preview is
a readable page with the real content, numbering, boilerplate and
translations. It is close, not exact: it is not split into pages like the
PDF, and images load only where the browser can reach them.

Previews are set up for everyone in the team configuration, one entry per
kind of document, matched by the document's root element (and optionally
by the schema it points to, `schema_location = "*Cover*.xsd"`, or its file
name, `file_name = "*-cover.xml"`; the first entry that matches is used):

```toml
[[preview]]
name = "Manual"
root = "book"
stylesheet = "stylesheets/book-to-fo.xsl"
html = "stylesheets/fo-to-html.xsl"
params = { output = "html" }
```

Without `html`, the stylesheet's own output opens in Xylem.

To try a preview without changing the team file, add one in File ›
Preferences › Stylesheets. Your own entries are tried before the team's.
The engine can be chosen there too.

The `html` step runs the way a publishing service runs a second step on text
it already has: the FO is read as it is and the HTML is written as that
stylesheet's `xsl:output` asks (its doctype, HTML rather than XML syntax).

## Not yet

- Previews of single modules: the stylesheets expect a whole publication
  (as the publishing system assembles it), so today the preview works on
  publication documents.
- An exact, paginated preview (a PDF from the publishing server).
- Automatic fixes for numbering and for searches through lists held in
  variables: both need a judgement call, so Analyze reports them as notes.
  `xylem-xslt-check --prove` checks such a change made by hand.
