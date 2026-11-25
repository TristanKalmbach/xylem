<?xml version="1.0" encoding="UTF-8"?>
<!-- An included module in another folder: its relative URIs resolve
     against its own location. -->
<xsl:stylesheet version="2.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
  <xsl:template name="module-relative">
    <doc><xsl:value-of select="document('data.xml')/data/v"/></doc>
    <doc2><xsl:value-of select="doc('data.xml')/data/v, doc-available('data.xml'), doc-available('inc.xsl')"/></doc2>
    <sbu><xsl:value-of select="ends-with(static-base-uri(), '/parity/sub/inc.xsl')"/></sbu>
  </xsl:template>
</xsl:stylesheet>
