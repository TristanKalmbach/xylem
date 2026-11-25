# Using Xylem's XSLT engine from another application

Xylem's XSLT 2.0 / XPath 2.0 engine (`crates/xylem-xslt-engine`) can run
stylesheets for other programs, such as a content management or publishing
system that runs Saxon or .NET's XSLT processor today. Nothing here is tied
to Xylem's editor: there is a C library and a command-line tool, and either
can be called from .NET, Java, Python, Node or a shell script.

| Way in | Best for | Where |
|---|---|---|
| **C library** `xylem_xslt` (`.dll` / `.so` / `.dylib`) | Running in-process, many transformations, lowest overhead | `crates/xylem-xslt-ffi`, header `crates/xylem-xslt-ffi/include/xylem_xslt.h` |
| **`xylem-xslt serve`** | Out of process: a crash or a hang cannot take the host down; no native interop | `crates/xylem-xslt-cli` |
| **`xylem-xslt transform`** | Scripts and one-off runs | `crates/xylem-xslt-cli` |

Release archives carry `xylem-xslt` and, under `lib/`, the library, its
header and this file. To build them yourself:

```
cargo build --release -p xylem-xslt-ffi      # target/release/xylem_xslt.dll (or .so, .dylib)
cargo build --release -p xylem-xslt-cli      # target/release/xylem-xslt(.exe)
```

## What the engine does

- Stylesheets whose root `version` is `2.0` (or higher) run as XSLT 2.0;
  others run with XSLT 1.0 rules (numbers as doubles, 1.0 comparisons).
- **Output.** *Publish* output (`XYLEM_OUTPUT_PUBLISH`, or `serialize`
  off) is what a .NET publishing service that hosts Saxon saves: the
  result tree written as `XmlDocument.OuterXml` would write it, with no
  XML declaration and `xsl:output` ignored. *Serialize* output
  (`XYLEM_OUTPUT_SERIALIZE`, `serialize: true`) follows `xsl:output`; for
  the HTML method it matches .NET's XSLT 1.0 processor writing to a string
  (doctype, `META` content-type, void elements, CR LF line ends).
- **Parameters** are name/value strings. Each sets the global `xsl:param`
  of exactly that name; a value is converted to the parameter's declared
  type when that is `xs:boolean`, `xs:integer`, `xs:decimal` or `xs:double`,
  and is a string otherwise.
- **Relative URIs** (`document()`, `xsl:include`, `xsl:import`, external
  entities) resolve against the stylesheet's folder, as a service that
  compiles from the stylesheet file does. The input document gets the
  stylesheet's folder as its base URI.
- **`generate-id()`** gives the same values as Saxon 9.7 (`d<doc>e<node>`).
- **Not supported yet:** `xsl:for-each-group`, `xsl:analyze-string`,
  `xsl:result-document`, `xsl:next-match`, `xsl:apply-imports`, schema
  awareness, output indentation. A stylesheet that uses them fails to
  compile with a message naming the instruction. See
  [design/xslt-engine.md](design/xslt-engine.md) and
  [design/conformance.md](design/conformance.md).

## The C library

```c
XylemStylesheet *ss = xylem_xslt_compile("C:/styles/book-to-fo.xsl");
if (!ss) { fprintf(stderr, "%s\n", xylem_xslt_last_error()); return 1; }

uint8_t *out; size_t out_len;
int rc = xylem_xslt_transform(ss, (const uint8_t *)xml, strlen(xml),
                              "{\"mode\":\"final\"}", XYLEM_OUTPUT_PUBLISH,
                              &out, &out_len);
if (rc == 0) {
    fwrite(out, 1, out_len, stdout);
    xylem_xslt_free_bytes(out, out_len);
} else {
    fprintf(stderr, "%s\n", xylem_xslt_last_error());
}
xylem_xslt_free(ss);
```

- Compile a stylesheet once and keep the handle; compiling the 38-module
  stylesheets this engine was built for takes about 30 ms.
- One handle can run many transformations at once on different threads:
  compile each stylesheet once and share the handle.
- `xylem_xslt_last_error_json()` gives the same error as JSON with the
  stylesheet `file`, `line` and `column` it refers to (where known) and,
  for run-time errors, the `stack` of templates and functions it passed
  through, innermost first; `serve` adds the same object as `at` to a
  failed reply. Rust callers get it as `XsltError`
  (`Stylesheet::compile_with`, `XsltError::from_message`).
- `xylem_xslt_last_error()` is per thread, and so is
  `xylem_xslt_last_log()`: after a transformation, a JSON object with its
  `xsl:message` texts and warnings about errors it recovered from.
- A `document()` that cannot be read fails an XSLT 1.0 transformation and,
  in XSLT 2.0, gives an empty sequence with a warning in the log, as the
  reference publishing service does; `doc()` always fails.
- No panic crosses the C boundary. `xylem_xslt_transform` returns 1 for
  bad arguments, 2 when the stylesheet or input fails (a dynamic error,
  malformed XML) and 3 for an internal engine failure; the handle stays
  usable after any of them.
- In publish mode a result that is not one well-formed document (text at
  the top level, even whitespace, or more than one top-level element)
  fails with return code 2, with the message the reference publishing
  service gives; `XYLEM_OUTPUT_SERIALIZE` writes such fragments.
- Add `XYLEM_NO_FILE_ACCESS` to the flags when the input comes from users:
  `document()` and external entities then fail instead of reading files.
  (Stylesheet modules are still read when compiling.)

### From C# (.NET Framework or .NET)

```csharp
using System;
using System.Runtime.InteropServices;
using System.Text;

public sealed class XylemStylesheet : IDisposable
{
    const string Lib = "xylem_xslt";   // xylem_xslt.dll next to the application
    public const uint Publish = 0, Serialize = 1, NoFileAccess = 2;

    [DllImport(Lib, CallingConvention = CallingConvention.Cdecl)]
    static extern IntPtr xylem_xslt_compile(byte[] pathUtf8);
    [DllImport(Lib, CallingConvention = CallingConvention.Cdecl)]
    static extern int xylem_xslt_transform(IntPtr ss, byte[] input, UIntPtr inputLen,
        byte[] paramsJsonUtf8, uint flags, out IntPtr output, out UIntPtr outputLen);
    [DllImport(Lib, CallingConvention = CallingConvention.Cdecl)]
    static extern void xylem_xslt_free_bytes(IntPtr ptr, UIntPtr len);
    [DllImport(Lib, CallingConvention = CallingConvention.Cdecl)]
    static extern void xylem_xslt_free(IntPtr ss);
    [DllImport(Lib, CallingConvention = CallingConvention.Cdecl)]
    static extern IntPtr xylem_xslt_last_error();

    IntPtr handle;

    public XylemStylesheet(string path)
    {
        handle = xylem_xslt_compile(Utf8(path));
        if (handle == IntPtr.Zero) throw new InvalidOperationException(LastError());
    }

    /// <summary>Runs the stylesheet; paramsJson is a JSON object or null.</summary>
    public string Transform(string inputXml, string paramsJson, uint flags = Publish)
    {
        var input = Encoding.UTF8.GetBytes(inputXml);
        int rc = xylem_xslt_transform(handle, input, (UIntPtr)input.Length,
            paramsJson == null ? null : Utf8(paramsJson), flags, out var ptr, out var len);
        if (rc != 0) throw new InvalidOperationException(LastError());
        try
        {
            var bytes = new byte[(int)len];
            Marshal.Copy(ptr, bytes, 0, bytes.Length);
            return Encoding.UTF8.GetString(bytes);
        }
        finally { xylem_xslt_free_bytes(ptr, len); }
    }

    public void Dispose()
    {
        if (handle != IntPtr.Zero) { xylem_xslt_free(handle); handle = IntPtr.Zero; }
    }

    static byte[] Utf8(string s) => Encoding.UTF8.GetBytes(s + "\0");

    static string LastError()
    {
        var p = xylem_xslt_last_error();
        if (p == IntPtr.Zero) return "unknown error";
        int n = 0;
        while (Marshal.ReadByte(p, n) != 0) n++;
        var b = new byte[n];
        Marshal.Copy(p, b, 0, n);
        return Encoding.UTF8.GetString(b);
    }
}
```

Build the library for the host's architecture (`x86_64-pc-windows-msvc` for
a 64-bit .NET process) and copy `xylem_xslt.dll` next to the application.
Parameters can be built with any JSON serializer (string values; numbers
and booleans are accepted too).

### A complete .NET host

[`tools/dotnet-embed`](../tools/dotnet-embed/README.md) is a .NET Framework
4 program, written in C# 5 and built with the `csc.exe` that ships with
Windows. It calls this library the way a .NET publishing service calls
Saxon-HE. It takes the same command line as `SaxonDotnetHost.exe`, so
`xylem-xslt-check differential --candidate` can compare the two on real
stylesheets and inputs. Its `selftest` command checks that the library is
safe inside a long-running .NET process: one handle shared by 8 threads,
no memory growth over many compile/transform/free cycles, and errors that
come back as exceptions. Its README has the results, timings against Saxon,
and the engine problems found. Two of those problems end the host process:
endless recursion, and input nested more than about 1,000 elements deep.

## The command line

```
xylem-xslt transform --stylesheet book-to-fo.xsl --input manual.xml --output manual.fo --param mode=final
xylem-xslt transform --stylesheet fo2html.xsl --input manual.fo --output manual.html --serialize
xylem-xslt compile --stylesheet a.xsl --stylesheet b.xsl
```

### `serve`: many transformations, one process

`xylem-xslt serve` reads one JSON request per line on standard input and
answers each with one JSON line on standard output. Compiled stylesheets
stay in memory (compiled again when the stylesheet file changes; send
`{"command":"clear"}` after changing an included module).

```
→ {"id":1,"stylesheet":"C:/s/book-to-fo.xsl","input":"C:/in/a.xml","output":"C:/out/a.fo","params":{"mode":"final"}}
← {"id":1,"ok":true,"ms":42,"messages":[]}
→ {"id":2,"stylesheet":"C:/s/book-to-fo.xsl","input_text":"<book/>"}
← {"id":2,"ok":true,"ms":3,"messages":[],"output_text":"<fo:root …>"}
→ {"id":3,"stylesheet":"C:/s/missing.xsl","input_text":"<a/>"}
← {"id":3,"ok":false,"error":"…"}
→ {"command":"quit"}
```

| Field | Meaning |
|---|---|
| `id` | Any JSON value, echoed in the reply |
| `stylesheet` | Stylesheet file |
| `input` / `input_text` | Input file, or the input document itself |
| `output` | File to write; without it the reply carries `output_text` |
| `params` | Object of parameter names and values |
| `serialize` | `true` for `xsl:output` serialization |
| `no_file_access` | `true` to refuse `document()` and external entities |

Requests are handled in order; run several `serve` processes for parallel
work.

## Moving a system from Saxon to this engine

1. **Keep both for a while.** Run the same stylesheets and inputs through
   both and compare: Xylem's Check tool does this (`xylem-xslt-check
   suite.toml --reference SaxonDotnetHost.exe`), or compare the outputs in
   your own tests. The engine was built against the reference service's
   saved outputs and matches them exactly on the stylesheets it was tried
   with, but other stylesheets may use features it does not have yet.
2. **Put the engine behind the system's own stylesheet interface** (the
   place that today compiles with Saxon or `XslCompiledTransform`), with a
   setting that chooses the engine per installation, so it can be switched
   back without a code change.
3. **Match parameter names exactly.** If the system passes parameter names
   in a different case from the stylesheet's `xsl:param`, they have no
   effect here (as with Saxon when the name is set as given).
4. **Choose the output form per step:** publish output for stylesheets
   whose result is stored or handed to an FO formatter as text; serialize
   output for steps that write HTML or other `xsl:output` formats.
5. **Turn file access off** for transformations of untrusted input.
