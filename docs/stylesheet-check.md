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

## The engine

Stylesheets run in a separate program that behaves like the publishing
service that runs them in production, so the output can be compared with
what that service saves. `tools/saxon-dotnet` has one for services that host
Saxon-HE on .NET; build it with your copy of the Saxon assemblies (see its
README). Point Xylem at it in the window's Engine field, or for everyone with
`xslt_host` in the team configuration (see [team-setup.md](team-setup.md)).

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

## From a terminal

```
xylem-xslt-check suite.toml [--stylesheet file.xsl]
                 [--against saved | committed[:<rev>] | other.xsl]
                 [--host SaxonDotnetHost.exe] [--work folder]
```

`--host` defaults to the `XYLEM_XSLT_HOST` environment variable. The exit
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

## Not yet

- Which templates the tests never reached (a change there is not proven by
  the check).
- Suggested fixes for slow patterns.
- A preview of the result for authors.
