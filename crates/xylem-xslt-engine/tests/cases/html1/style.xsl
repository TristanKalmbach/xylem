<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE xsl:stylesheet [
  <!ENTITY title-call "<xsl:call-template name='title' xmlns:xsl='http://www.w3.org/1999/XSL/Transform'/>">
]>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
  <xsl:output method="html" version="4.0" doctype-public="-//W3C//DTD HTML 4.0 Transitional//EN" indent="no"/>
  <xsl:template match="/">
    <html>
      <head>&title-call;<style type="text/css">
        a &gt; b { color: red; }
      </style></head>
      <body>
        <xsl:apply-templates select="book/chapter"/>
        <p>
          <xsl:value-of select="10 div 4"/>|<xsl:value-of select="7 div 3"/>|<xsl:value-of select="1 div 0"/>|<xsl:value-of select="'3' + 4"/>|<xsl:value-of select="//part/@price"/>|<xsl:value-of select="sum(//part/@price)"/>|<xsl:value-of select="count(//para) * 1.5"/>
        </p>
        <p>
          <xsl:value-of select="//chapter/@n = 10"/>|<xsl:value-of select="//chapter/@n &gt; '5'"/>|<xsl:value-of select="'abc' = 'abc'"/>|<xsl:value-of select="//chapter = true()"/>|<xsl:value-of select="string-length(normalize-space('&#xA0; x &#xA0;'))"/>
        </p>
        <br/><hr/>
        <img src="images/caf&#xE9; photo.png" alt="a &amp; b &lt; c"/>
        <a href="page.html?a=1&amp;b=2">link</a>
        <div class="first">
          <xsl:attribute name="id">x</xsl:attribute>
          <xsl:attribute name="class">second</xsl:attribute>
          <xsl:text>line one
line two</xsl:text>
        </div>
      </body>
    </html>
  </xsl:template>
  <xsl:template name="title"><title>Book</title></xsl:template>
  <xsl:template match="chapter">
    <h1><xsl:value-of select="title"/> &amp; more &lt;x&gt;</h1>
  </xsl:template>
</xsl:stylesheet>
