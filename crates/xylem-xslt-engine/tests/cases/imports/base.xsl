<xsl:stylesheet version="2.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
  <xsl:variable name="who" select="'base'"/>
  <xsl:template match="title"><base-title><xsl:value-of select="."/></base-title></xsl:template>
  <xsl:template match="para"><base-para/></xsl:template>
  <xsl:template name="sig"><sig from="base"/></xsl:template>
  <xsl:template match="warning" priority="10"><base-warning/></xsl:template>
</xsl:stylesheet>
