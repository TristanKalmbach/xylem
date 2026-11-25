# .NET host for the C library

`XylemDotnetHost.exe` is a small .NET Framework program that runs stylesheets
on Xylem's built-in XSLT engine through its C library (`xylem_xslt.dll`, see
[docs/embedding.md](../../docs/embedding.md)), called the way a .NET
publishing service calls Saxon-HE and `XslCompiledTransform` today. It shows
that the engine can stand in for Saxon in such a service without changing
how the service hands over inputs, parameters and outputs.

It takes the same command line as
[`SaxonDotnetHost.exe`](../saxon-dotnet/README.md) and prints the same JSON
lines, so `xylem-xslt-check` can drive both and compare their outputs.

## Build

You need Windows (the .NET Framework 4 C# compiler ships with it) and Rust.

```
cargo build --release -p xylem-xslt-ffi
tools\dotnet-embed\build.cmd
```

`build.cmd` compiles `XylemDotnetHost.cs` with `csc.exe` (C# 5, no NuGet, no
SDK) into `bin\` and copies `target\release\xylem_xslt.dll` next to it. Give
it another folder holding `xylem_xslt.dll` as its argument to use a different
build of the library. `bin\` is not committed.

### Bitness

The service this stands in for is built `AnyCPU` for release without
`Prefer32Bit` (MSBuild leaves it off when the project does not set it). Its
other configurations set `PlatformTarget` to `x64`, which ignores
`Prefer32Bit`. On 64-bit Windows the service is therefore a 64-bit process
and loads a 64-bit DLL. `build.cmd` builds this program the same way
(`/platform:anycpu`, no 32-bit preference) with the 64-bit compiler, so it
loads the `x86_64-pc-windows-msvc` library that `cargo build` makes by
default. The self-tests print the process's bitness (`"bits":64`). A 32-bit
process needs a library built with `--target i686-pc-windows-msvc`.

## Run

```
bin\XylemDotnetHost.exe run --stylesheet book-to-fo.xsl ^
    --job manual.xml=manual.fo --param mode=final [--repeat 5]
bin\XylemDotnetHost.exe run --serialize --stylesheet fo2html.xsl ^
    --job manual.fo=manual.html
bin\XylemDotnetHost.exe compile --stylesheet a.xsl --stylesheet b.xsl
```

Each job prints `{"job":…,"input":…,"output":…,"ok":…,"ms":…}` (plus
`runs_ms` with `--repeat`, `error` on failure), then a summary line
`{"summary":true,"xslt":…,"compiled":…,"compile_ms":…,"jobs":…,"failed":…}`.
`--allow-scripts` is accepted and ignored (the engine never runs embedded
scripts). `--profile` is accepted, but the C library has no template timings,
so no profile is written.

It feeds the engine the way the service feeds its engines:

| Step | Input handed over | Output |
|---|---|---|
| XSLT 2.0 stylesheet | the input file's bytes | `XYLEM_OUTPUT_PUBLISH` (`XmlDocument.OuterXml` form) |
| XSLT 1.0 stylesheet | the input read into an `XDocument`, as its string | `XYLEM_OUTPUT_PUBLISH` |
| `--serialize`, XSLT 2.0 | the input file's bytes | `XYLEM_OUTPUT_SERIALIZE` (as `xsl:output` asks) |
| `--serialize`, XSLT 1.0 (FO to HTML) | the input file read as text | `XYLEM_OUTPUT_SERIALIZE` |

The stylesheet is compiled once from its file, so includes, imports and
`document()` resolve next to it. Every result is written UTF-8 with a byte
order mark, as the service saves it. Parameters are passed with the names
given and converted by the engine to a declared `xs:boolean` or numeric type.
SaxonDotnetHost also converts to `xs:float`, `xs:anyURI` and `xs:QName`;
the engine passes those as strings.

## Comparing with Saxon

```
cargo build --release -p xylem-xslt
tools\saxon-dotnet\build.cmd "<folder holding saxon9he-api.dll>"
target\release\xylem-xslt-check.exe differential <stylesheet.xsl> ^
    --inputs <folder> --input-pattern "*.xml" ^
    --reference tools\saxon-dotnet\bin\SaxonDotnetHost.exe ^
    --candidate tools\dotnet-embed\bin\XylemDotnetHost.exe ^
    --params "mode=final" --work %TEMP%\diff-<name>
```

Add `--serialize` for an FO-to-HTML step. Use a new `--work` folder for each
run: the check carries on from the `results.jsonl` it finds there.

### Results (library 0.6.0)

These checks used 83 saved publications from a production service: the
assembled XML for the publishing stylesheets, and the saved XSL-FO for the
HTML step. The stylesheets are four XSLT 2.0 publication stylesheets (many
modules each, XML to XSL-FO) and the service's XSLT 1.0 FO-to-HTML
stylesheet.

| Stylesheet | Cases | Same | Byte-identical | Different | Only one failed |
|---|---:|---:|---:|---:|---:|
| Publication stylesheet A | 83 | 83 | 8 | 0 | 0 |
| Publication stylesheet B | 83 | 83 | 39 | 0 | 0 |
| Publication stylesheet C | 83 | 83 | 8 | 0 | 0 |
| Publication stylesheet D | 83 | 83 | 8 | 0 | 0 |
| FO to HTML (`--serialize`) | 83 | 83 | 83 | 0 | 0 |

All 415 outputs agree. None of these stylesheets takes the XSLT 1.0 path
without `--serialize`. That path was tried only with a small stylesheet of
its own, which shows two differences in the engine (problem 7 below). Every output that is not byte-identical differs only
in the document number inside `generate-id()` values. Saxon's counter keeps
rising while its process runs several jobs, and Check renumbers those
counters before comparing (see the Saxon host's README). The engine
numbers documents the same way on every run.

## Checks for running in-process

```
bin\XylemDotnetHost.exe selftest threads|memory|errors|all ^
    [--stylesheet X.xsl --input Y.xml --param name=value ...] ^
    [--threads 8] [--rounds 4] [--iterations 1000] [--only compile|transform] ^
    [--reference ..\saxon-dotnet\bin\SaxonDotnetHost.exe]
```

Without `--stylesheet`/`--input` the checks use a small stylesheet and input
of their own: an include, `document()`, keys, sorting, `xsl:number`,
functions, `generate-id()` and typed parameters. Each check prints JSON
lines and exits 0 when it passed. Known engine problems are labelled `xfail`
and do not fail the run. A known problem that starts working shows as
`xpass`.

- **threads**: one compiled stylesheet is used from 8 .NET threads at once
  (started together on a barrier, default 1 MB stacks). Every result is
  compared byte for byte with a single-threaded run. The same is done with
  one handle per thread.
- **memory**: compile, transform and free, 1000 times by default (the
  heap is still settling for the first few hundred). Private bytes
  are sampled after a full garbage collection. The peak of the last third of
  the samples must stay within 4 MB (or 5 %) of the peak of the middle
  third. `--only compile` and `--only transform` (one handle kept) separate
  the two.
- **errors**: each case runs in a child process (this program again) on a
  default-stack worker thread, with crash dialogs suppressed. A crash
  therefore shows up as the child's exit code and does not end the check.
  With `--reference`, the same case also runs through SaxonDotnetHost.

### Results (library 0.6.0, 64-bit, CLR 4.0.30319.42000)

Results were measured on the engine as of `be6fd8c` on main, except where
marked.

**Threads.** The built-in sample (4 rounds) and a publication stylesheet on
the largest input (1.5 MB, 2 rounds) both passed.

| Check | Runs | Identical | Wall time |
|---|---:|---:|---:|
| sample, shared handle | 32 | 32 | 3.18 s (single run 95 ms) |
| sample, handle per thread | 32 | 32 | 0.74 s |
| publication, shared handle | 16 | 16 | 5.38 s (single run 333 ms) |
| publication, handle per thread | 16 | 16 | 0.91 s |

A shared handle is safe, but its calls take turns: 8 threads on one handle
take as long as running one after another. For parallel work, keep one
compiled stylesheet per worker thread. That scales about 4–6× on 8 threads.

**Memory.** Private bytes level off and stay there.

| Cycle | Input | Iterations | Private bytes (first → peak, middle → last third) |
|---|---|---:|---|
| compile + transform + free | sample | 1200 | 20.5 MB → 27.5 → 27.5 MB |
| compile + transform + free | 49 KB publication | 1500 | 42.4 MB → 52.9 → 52.8 MB |
| compile + transform + free | 1.5 MB publication | 300 | 34.8 MB → 72.4 → 72.3 MB ³ |
| compile + free | publication stylesheet | 1500 | 20.9 MB → 30.9 → 32.1 MB ³ |
| transform (one handle) | 49 KB publication | 1500 | 29.0 MB → 52.8 → 53.4 MB ³ |

³ Measured on the engine as of `9d47447`, before the rebase.

With the 1.5 MB input, single samples swing between 35 and 72 MB, depending
on whether the heap has returned the last run's freed tree to Windows. The
peaks do not rise.

**Errors.**

| Case | Engine through .NET | Saxon through .NET | Verdict |
|---|---|---|---|
| `xsl:message terminate="yes"` | error | error | pass |
| `fn:error()` | error | error | pass |
| dynamic type error | error | error | pass |
| XPath that does not parse | compile error | compile error | pass |
| input not well-formed | error | error | pass |
| endless named-template recursion | **process ends, 0xC00000FD** | process ends, 0xC00000FD | xfail |
| endless `xsl:function` recursion | **process ends, 0xC00000FD** | process ends, 0xC00000FD | xfail |
| endless `apply-templates` recursion | **process ends, 0xC00000FD** | process ends, 0xC00000FD | xfail |
| `document()` of a missing file | empty, no error | empty, no error | xfail |
| `doc()` of a missing file | **empty, no error** | error (FODC0002) | xfail |
| input nested 2000 elements deep | **process ends, 0xC00000FD** | no crash | xfail |

## Speed

The tables below compare Saxon-HE 9.7 through .NET with the engine through
.NET on the largest input (`--repeat 5`, median of five runs, same machine,
one after the other). The in-process Rust column is `xylem-xslt-check run`,
the engine without .NET. Its numbers show that calling the engine through
P/Invoke costs about 3–5 %.

| Stylesheet (input) | Saxon compile | Saxon run | Engine compile | Engine run (.NET) | Engine run (Rust) |
|---|---:|---:|---:|---:|---:|
| Publication A (1.5 MB XML) | 1219 ms | 222 ms² | 33 ms | 303 ms | 287 ms |
| Publication B (1.5 MB XML) | 1054 ms | 23 ms | 33 ms | 33 ms | 31 ms |
| Publication C (1.5 MB XML) | 982 ms | 115 ms | 27 ms | 228 ms | 217 ms |
| Publication D (1.5 MB XML) | 975 ms | 138 ms | 26 ms | 287 ms | 275 ms |
| FO to HTML, XSLT 1.0 (2.3 MB FO)¹ | 21 ms | 39 ms | 3 ms | 1464 ms | 1527 ms |

¹ The reference here is `XslCompiledTransform`, not Saxon.
² 142 ms in an earlier run on the same machine; Saxon's times vary more
between runs than the engine's.

Saxon's first run is slower (about 270 ms) than the median shown. A service
that compiles a stylesheet for each publication pays Saxon's compile time
every time: about 1.2 s per publication, against about 0.3 s on the engine.
With a cached compiled stylesheet, the engine runs the publishing
stylesheets about 2× slower than Saxon. It runs the FO-to-HTML step about
30× slower than `XslCompiledTransform`.

## Engine problems found

These are reported, not fixed here. The `errors` check keeps each one as an
expected failure.

1. **No recursion limit.** Endless recursion through named templates,
   functions or `apply-templates` overflows the native stack. Windows then
   ends the whole host process with 0xC00000FD. `catch_unwind` cannot catch
   this, so no error comes back to .NET. Saxon on .NET ends the same way, so
   this is not a regression against the current service. It still means a
   faulty stylesheet takes the service down. Repro: `selftest errors`, cases
   `recursion-*`. The stylesheet is

   ```xml
   <xsl:template match="/"><xsl:call-template name="down"><xsl:with-param name="n" select="1"/></xsl:call-template></xsl:template>
   <xsl:template name="down"><xsl:param name="n"/><level n="{$n}"><xsl:call-template name="down"><xsl:with-param name="n" select="$n + 1"/></xsl:call-template></level></xsl:template>
   ```

2. **Deeply nested input ends the process.** An input about 1,150 elements
   deep overflows a 1 MB stack, even with a stylesheet that only writes
   `<r/>`. Saxon does not crash on it, nor on a 100,000-deep input. `xylem-xslt-check run` fails
   the same way, so the cause is the input parser or tree, not .NET. For a
   service that takes uploaded content, this is a denial of service. Repro:
   `selftest errors`, case `input-nested-2000`, or any `<e><e>…</e></e>`
   input 1,200 levels deep with the stylesheet
   `<xsl:template match="/"><r/></xsl:template>`.
3. **`fn:doc()` of a missing file returns an empty sequence.** It must raise
   `FODC0002`, as Saxon does. Repro: case `doc-missing`, with
   `<xsl:copy-of select="doc('does-not-exist.xml')"/>`.
4. **`document()` of a missing file is not an error.** It gives an empty
   node-set. XSLT 2.0 makes this a recoverable error, and Saxon recovers the
   same way, so the output matches. A host that wants a failure has no way
   to ask for one, apart from `XYLEM_NO_FILE_ACCESS`, which refuses every
   file. Case `document-missing`.
5. **Compile errors name the stylesheet with a `\\?\` prefix**
   (`\\?\C:\…\a.xsl (xsl:value-of): unexpected Eof in '1 +'`). This is a
   canonicalized Windows path where the path as given would read better.
6. **Speed of the XSLT 1.0 FO-to-HTML step** (see Speed). The engine's
   profile (`xylem-xslt-check run --profile`) puts most of the time in the
   self time of the two templates that call `apply-templates` over most of
   the elements. A likely cause, not yet confirmed, is matching template
   rules whose patterns have predicates (a `preceding-sibling` step and a
   `generate-id()` comparison) against every element.
7. **XSLT 1.0 publish output is not what `XslCompiledTransform` writes.**
   The service writes a 1.0 stylesheet's result through an `XmlWriter`, and
   the engine's publish output differs from it in two ways:
   - `generate-id()` has Saxon's form (`d1e6`) instead of .NET's (`ID0EG`).
   - Newlines in text stay LF instead of becoming CR LF (the writer's
     default `NewLineHandling.Replace`).

   The harness hands the engine the same `XDocument` text the service hands
   `XslCompiledTransform`, so node positions match. Repro: a
   `version="1.0"` stylesheet that writes `generate-id()` and copies
   indented input, run through `differential` against SaxonDotnetHost
   without `--serialize`. The FO-to-HTML step (`--serialize`) is not
   affected.
