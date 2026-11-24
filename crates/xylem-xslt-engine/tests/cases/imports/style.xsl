<xsl:stylesheet version="2.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
  <xsl:import href="base.xsl"/>
  <xsl:include href="common.xsl"/>
  <xsl:strip-space elements="*"/>
  <xsl:variable name="who" select="'top'"/>
  <xsl:template match="/"><r who="{$who}"><xsl:apply-templates select="//chapter[1]"/><xsl:call-template name="sig"/></r></xsl:template>
  <xsl:template match="para[em]"><top-para><xsl:apply-templates/></top-para></xsl:template>
  <xsl:template match="warning"><top-warning/></xsl:template>
  <xsl:template match="section"/>
</xsl:stylesheet>
