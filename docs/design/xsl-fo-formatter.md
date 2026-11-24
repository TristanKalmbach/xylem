# XSL-FO formatting: scope note

Status: scoping only. Nothing here is being built yet.

The publishing pipeline is: publishing stylesheet → XSL-FO → a commercial
FO formatter on the server → PDF. Xylem's Publication preview stops at FO
and converts it to HTML with the team's FO-to-HTML stylesheet, which is
good for checking content and structure but not for layout. An "exact
preview" means showing pages as the server's formatter would print them.
That is a separate and much larger project than the XSLT engine.

## What an exact preview needs

1. **The FO object model and property engine.** About 60 formatting
   objects and some 250 properties, with inheritance, shorthand expansion,
   percentages, `from-parent()` / `inherited-property-value()` and the
   expression language, and the conditional and `.minimum/.optimum/
   .maximum` components that the target FO uses heavily
   (`space-before.conditionality`, `keep-together.within-page`, …).
2. **Fonts and text shaping.** The same fonts as the server (licensing
   matters), OpenType shaping, kerning and ligatures, font fallback,
   bidirectional text for some languages. Pure-Rust options exist
   (`ttf-parser`/`skrifa` for fonts, `rustybuzz` for shaping).
3. **Line breaking and hyphenation.** UAX #14 line breaking, a Knuth–Plass
   or similar paragraph builder, hyphenation dictionaries per language,
   `text-align="justify"`, `widows`/`orphans`, `linefeed-treatment`,
   `white-space-collapse`.
4. **Page layout.** Page masters and sequences (odd/even/first/blank
   pages), regions, flows and static content, columns, keeps and breaks,
   space resolution between blocks, footnotes, floats/side floats, markers
   (`fo:retrieve-marker` for running headers), page-number citations
   (needs a second pass), `fo:leader` for tables of contents.
5. **Tables.** Fixed and automatic column widths,
   `proportional-column-width()`, spans, borders with the collapsing model,
   headers and footers repeated across pages, rows split across pages.
6. **Graphics.** SVG (the target documents embed many SVG diagrams),
   raster images with resolution handling, scaling and `content-width`.
7. **Vendor extensions.** The target stylesheets emit the server
   formatter's own extension namespace (`axf:` properties such as outline
   levels and titles, and others). An exact match has to reproduce those
   documented behaviours too.
8. **Output.** Either render pages to the screen (and to PDF for printing),
   with a hit map from page positions back to FO and source nodes so the
   preview can link to the editor.

"Exact" here means: same line breaks and page breaks as the server's PDF.
That requires matching the server formatter's choices in every area above,
including undocumented heuristics (line breaking, keep resolution, table
width negotiation). An oracle is available — the saved PDFs — but
comparing PDFs at the layout level (positions of text runs per page) needs
tooling of its own.

## Size and options

- **Full formatter in Rust.** Comparable projects (Apache FOP, a Java
  formatter that does not match commercial output exactly) are several
  hundred thousand lines. Even a focused subset for these stylesheets is
  likely 40,000–80,000 lines plus font and hyphenation data — an order of
  magnitude beyond the XSLT engine.
- **Approximate page preview.** Pages with the right page masters, fonts,
  keeps and tables but no guarantee of identical breaks. Much smaller
  (perhaps 15,000–25,000 lines) and still useful for spotting layout
  regressions, but not "exact".
- **Keep using the server.** Send FO to the server formatter for exact
  PDFs when online; use the HTML preview offline. No engine work.

## Suggested next steps (when this is picked up)

1. Measure which FO objects, properties and `axf:` extensions the target
   FO output actually uses (the oracle suites give hundreds of real FO
   files).
2. Build a layout-level PDF comparison (text runs and their page and
   position) against the saved PDFs, so any formatter work has a measure.
3. Decide between "approximate preview" and "exact" with those numbers.
