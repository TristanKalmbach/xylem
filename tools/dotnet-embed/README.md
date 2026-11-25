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

### Results (64-bit process, CLR 4.0.30319.42000)

Measured on main as of the engine's recursion, static-check and speed
work.

**Threads.** One compiled stylesheet runs on many threads at once:

| Check | Runs | Identical | Wall time |
|---|---:|---:|---:|
| sample, shared handle, 8 threads | 32 | 32 | 66 ms (single run 17 ms) |
| sample, handle per thread, 8 threads | 32 | 32 | 61 ms |

**Memory.** 1,000 compile + transform + free cycles: private bytes 44.1 MB
at the start, peaking at 47.5 MB in both the middle and the last third
(0.0 MB growth), every output identical.

**Errors.** Every case comes back to .NET as the reference host behaves, and
none ends the process:

| Case | Engine through .NET | Saxon through .NET |
|---|---|---|
| `xsl:message terminate="yes"` | error | error |
| `fn:error()` | error | error |
| dynamic type error | error | error |
| XPath that does not parse | compile error | compile error |
| input not well-formed | error | error |
| endless named-template recursion | error (XTDE0000, after 10,000 nested calls) | **process ends, 0xC00000FD** |
| endless `xsl:function` recursion | error (XTDE0000) | **process ends, 0xC00000FD** |
| endless `apply-templates` recursion | error (XTDE0000) | **process ends, 0xC00000FD** |
| `document()` of a missing file | empty, with a warning | empty |
| `doc()` of a missing file | error (FODC0002) | error |
| input nested 2,000 elements deep | result | result |

Inputs nested 100,000 deep are read; ordinary recursion (an identity
transform over a deeply nested document) works to well beyond the depth
Saxon reaches before its stack overflows.

## Speed

Saxon-HE 9.7 through .NET against the engine through .NET, on the largest
input (a 1.4 MB publication; `--repeat 5`, median, same machine, one after
the other). The engine's output equals Saxon's on this input for all four
stylesheets.

| Stylesheet | Saxon compile | Saxon run | Engine compile | Engine run |
|---|---:|---:|---:|---:|
| Publication A | 1,073 ms | 309 ms | 24 ms | 12 ms |
| Publication B | 1,120 ms | 303 ms | 21 ms | 8 ms |
| Publication C | 985 ms | 326 ms | 20 ms | 11 ms |
| Publication D | 1,139 ms | 275 ms | 22 ms | 13 ms |
| FO to HTML, XSLT 1.0 (2.3 MB FO)¹ | 23 ms | 38 ms | 4 ms | 56 ms |

¹ The reference here is `XslCompiledTransform`, not Saxon.

Saxon's times vary between runs more than the engine's, and one input is
not typical. Over all 43 publications with saved production output (four
stylesheets, median of three runs each), the transforms take 533 ms in total
on the engine against 776 ms on Saxon: about 1.5× faster. Compiling is about
50× faster (20–25 ms against about a second), which matters to a service
that compiles a stylesheet per publication. The FO-to-HTML step runs about
1.5× slower than .NET's own XSLT 1.0 processor.

**What this means for publishing time:** Saxon takes at most about 0.3 s of
a publication here. Replacing it does not make publishing noticeably faster;
where a publication is slow, the time is spent elsewhere.

**Threads, real publication.** One compiled publication stylesheet on the
largest input, 8 threads × 2 rounds on a shared handle: 16 runs, all
identical, 38 ms in all against 17 ms for a single run.

## Known differences

- XSLT 1.0 numbers converted to text may differ from .NET's in the last
  digit (`6.4990000000000006` against `6.4990000000000005`). Both strings
  are the same double; the engine writes the shortest form that reads back
  exactly.
- `generate-id()` in XSLT 1.0 has the engine's form (`d1e6`), not .NET's
  (`ID0EG`). Within an output every ID still matches its references; use
  `xylem-xslt-check differential --rename-ids` to compare such outputs.
