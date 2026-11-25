<?xml version="1.0" encoding="UTF-8"?>
<!-- Shared templates, included by manual-to-html.xsl. -->
<xsl:stylesheet version="2.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
  <xsl:template name="number">
    <xsl:number count="chapter" format="1."/>
  </xsl:template>

  <xsl:template match="b"><strong><xsl:apply-templates/></strong></xsl:template>
  <xsl:template match="i"><em><xsl:apply-templates/></em></xsl:template>
  <xsl:template match="sup"><sup><xsl:apply-templates/></sup></xsl:template>
  <xsl:template match="code"><code><xsl:apply-templates/></code></xsl:template>
  <xsl:template match="table">
    <table>
      <caption><xsl:value-of select="title"/></caption>
      <xsl:for-each select="tgroup/*/row">
        <tr><xsl:for-each select="entry"><td><xsl:apply-templates/></td></xsl:for-each></tr>
      </xsl:for-each>
    </table>
  </xsl:template>
</xsl:stylesheet>
