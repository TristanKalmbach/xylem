<xsl:stylesheet version="2.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
    xmlns:xs="http://www.w3.org/2001/XMLSchema" exclude-result-prefixes="xs">
  <xsl:strip-space elements="*"/>
  <xsl:param name="mode"/>
  <xsl:param name="MODE" select="'unset'"/>
  <xsl:param name="pageCount" as="xs:integer" select="0"/>
  <xsl:variable name="terms" select="document('terms.xml')/terms"/>
  <xsl:template match="/">
    <edge mode="{$mode}" MODE="{$MODE}" string="{$mode instance of xs:string}" pages="{$pageCount * 2}">
      <xsl:variable name="empty" as="xs:string">
        <xsl:value-of select="''"/>
      </xsl:variable>
      <empty len="{string-length($empty)}" count="{count($empty)}"/>
      <terms n="{count($terms/term)}"><xsl:value-of select="$terms/term" separator="|"/></terms>
      <xsl:apply-templates/>
    </edge>
  </xsl:template>
  <xsl:template match="para"><p><xsl:value-of select="."/></p></xsl:template>
  <xsl:template match="step">
    <s n="{string-length(.)}"><xsl:number count="step[para]" from="step" format="1." level="single"/><xsl:apply-templates select="step"/></s>
  </xsl:template>
  <xsl:template match="comment()"><c id="{generate-id()}"><xsl:value-of select="."/></c></xsl:template>
</xsl:stylesheet>
