# Saxon .NET host

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

## Comparing output

IDs made by `generate-id()` look like `d31e20`. Saxon numbers documents with a
counter that keeps rising while the program runs, and a service that caches
compiled stylesheets keeps one counter across publishes, so the number after
`d` differs between runs while the rest of the output is the same. Before
comparing, renumber those counters in order of first appearance
(`d31e20` and `d7709e20` both become `d#1e20`). The number after `e` is the
node's position and stays the same.
