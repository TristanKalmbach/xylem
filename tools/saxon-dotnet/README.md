# Saxon .NET host

> **Optional.** Xylem runs stylesheets on its own built-in engine and does
> not need this program. Build it only to compare Xylem's results with
> Saxon's (Preferences › Stylesheets › Compare with, `xslt_reference`, or
> `xylem-xslt-check --reference`), or to run stylesheets on Saxon instead.

`SaxonDotnetHost.exe` runs XSLT stylesheets the way a .NET publishing service
that hosts Saxon-HE does, so output made here can be compared with output
that service saved. Xylem's stylesheet tools use it to check that a changed
stylesheet still produces the same result.

It copies these details of such a host:

- XSLT 2.0 stylesheets (root `version="2.0"`) run on Saxon-HE, compiled from
  the stylesheet's file, so imports, includes and `document()` resolve next to
  it. Other stylesheets run on .NET's `XslCompiledTransform`.
- The input's base URI is the stylesheet's folder, without a trailing slash.
- Parameters are matched case-insensitively against the declared `xsl:param`s
  and converted to their declared types.
- The result is written as the `OuterXml` of a .NET `XmlDocument`, UTF-8 with
  a byte order mark; `xsl:output` has no effect.
- XSLT 1.0 stylesheets run with `document()` enabled and DTDs parsed. The
  input is read into an `XDocument` and handed over as a string (so it has no
  base URI), and the XML declaration is left out. Embedded scripts
  (`msxsl:script`) are refused unless you pass `--allow-scripts`.

`--serialize` runs a stylesheet the way such a service runs a second step on
text it already has, such as XSL-FO to HTML: the input file is read as it is,
and the result is written as the stylesheet's `xsl:output` asks (method,
doctype, indentation) instead of as an `XmlDocument`. Xylem's publication
preview uses it for its HTML step.

## Build

You need Windows (the .NET Framework 4 C# compiler ships with it) and the
Saxon-HE .NET assemblies the publishing service uses, for example from its
NuGet package:

```
build.cmd "C:\path\to\Saxon-HE.9.7.0.21\lib\net40"
```

The program and a copy of the Saxon assemblies go to `bin\`, which is not
committed. Saxon-HE is published by Saxonica under the Mozilla Public License
2.0.

## Run

```
bin\SaxonDotnetHost.exe run --stylesheet stylesheets\book-to-fo.xsl ^
    --job manual.xml=manual.fo ^
    --param draft=yes
```

`--job input=output` can be repeated; the stylesheet is compiled once for all
of them. Each job prints one line of JSON (`ok`, time in `ms`, `error`), and a
summary line follows with the compile time. The exit code is 0 only when every
job worked.

`--repeat N` runs each job N times and reports the median time (`ms`) and
every run's time (`runs_ms`), for comparing speed before and after a change.

## Compiling only

```
bin\SaxonDotnetHost.exe compile --stylesheet a.xsl --stylesheet b.xsl
```

compiles each stylesheet as a run would and prints one JSON line per
stylesheet (`ok`, `ms`, `error`). The exit code is 0 only when all compiled.

## Timing templates

`--profile` times every template and function and writes
`<output>.profile.json` next to each output: for each, its kind, name or
match pattern, mode, file and line, how often it ran, the time spent in it
alone (`self_ms`) and including what it called (`total_ms`). Only XSLT 2.0
stylesheets can be timed.

With timing compiled in, this Saxon version evaluates global variables
before a streamed input is attached, so a profiled run builds the input tree
first, stripping whitespace exactly as the stylesheet asks. The output is the
same as an ordinary run's.

## Comparing output

IDs made by `generate-id()` look like `d31e20`. Saxon numbers documents with a
counter that keeps rising while the program runs, and a service that caches
compiled stylesheets keeps one counter across publishes, so the number after
`d` differs between runs while the rest of the output is the same. Before
comparing, renumber those counters in order of first appearance
(`d31e20` and `d7709e20` both become `d#1e20`). The number after `e` is the
node's position and stays the same.
