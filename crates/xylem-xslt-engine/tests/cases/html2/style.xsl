<?xml version="1.0" encoding="UTF-8"?>
<!-- The HTML output method indents by default in XSLT 1.0 (no indent
     attribute), the way .NET's HTML writer does: block elements on their
     own lines, inline elements and mixed content on the line. -->
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
  <xsl:output method="html" doctype-public="-//W3C//DTD HTML 4.0 Transitional//EN"/>
  <xsl:template match="/">
    <html>
      <head><title>T</title><style>p {}</style></head>
      <body>
        <div>
          <p>Text <b>bold</b> and <span>span</span>.</p>
          <p><b>only inline</b></p>
          <div><div>deep</div></div>
          <table><tr><td>a</td><td><p>block in cell</p></td></tr></table>
          <ul><li>one</li><li><a href="x">link</a></li></ul>
          <dl><dt>t</dt><dd>d</dd></dl><select><option>o</option></select><ol><li>a</li></ol>
          <form><input/></form>
          <hr>after a void element</hr><hr/>
          <font>f</font><s>s</s><em>e</em><h1>h</h1>
          <br/><img src="i.png"/><pre>  pre
  text</pre><script>var x;</script>tail text</div>
        <p/>
        <div><xsl:value-of select="''"/></div>
      </body>
    </html>
  </xsl:template>
</xsl:stylesheet>
