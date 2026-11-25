<?xml version="1.0" encoding="UTF-8"?>
<!-- Turns samples/manual-sample.xml into a simple HTML page. A sample for
     Tools > Run stylesheet, live stylesheet errors and F12 references. -->
<xsl:stylesheet version="2.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
  <xsl:include href="common.xsl"/>
  <xsl:output method="html" indent="yes"/>
  <xsl:param name="mode" select="'final'"/>

  <xsl:template match="/manual">
    <html>
      <head><title><xsl:value-of select="title"/></title></head>
      <body>
        <h1><xsl:value-of select="title"/></h1>
        <xsl:if test="$mode = 'draft'"><p class="banner">Draft</p></xsl:if>
        <xsl:apply-templates select="chapter"/>
      </body>
    </html>
  </xsl:template>

  <xsl:template match="chapter">
    <section id="{@id}">
      <h2><xsl:call-template name="number"/><xsl:text> </xsl:text><xsl:value-of select="title"/></h2>
      <xsl:apply-templates select="* except title"/>
    </section>
  </xsl:template>

  <xsl:template match="para"><p><xsl:apply-templates/></p></xsl:template>
  <xsl:template match="list[@type = 'numbered']"><ol><xsl:apply-templates/></ol></xsl:template>
  <xsl:template match="list"><ul><xsl:apply-templates/></ul></xsl:template>
  <xsl:template match="item"><li><xsl:apply-templates/></li></xsl:template>
  <xsl:template match="warning">
    <div class="warning"><strong>Warning</strong><xsl:apply-templates/></div>
  </xsl:template>
  <xsl:template match="xref">
    <a href="#{@linkend}"><xsl:apply-templates/></a>
  </xsl:template>
</xsl:stylesheet>
