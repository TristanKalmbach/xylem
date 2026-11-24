<xsl:stylesheet version="2.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
    xmlns:fo="http://www.w3.org/1999/XSL/Format" xmlns:xs="http://www.w3.org/2001/XMLSchema"
    exclude-result-prefixes="xs">
  <xsl:strip-space elements="*"/>
  <xsl:variable name="chapters" select="/book/chapter"/>
  <xsl:template match="/">
    <fo:root>
      <xsl:apply-templates/>
    </fo:root>
  </xsl:template>
  <xsl:template match="book">
    <fo:block id="{@id}" role="book-{@lang}">
      <xsl:value-of select="title"/>
      <xsl:text> (</xsl:text>
      <xsl:value-of select="count($chapters)"/>
      <xsl:text> chapters)</xsl:text>
    </fo:block>
    <xsl:apply-templates select="chapter | appendix"/>
    <xsl:apply-templates select="chapter" mode="toc"/>
    <xsl:apply-templates select="comment()"/>
  </xsl:template>
  <xsl:template match="chapter">
    <xsl:variable name="pos" select="position()"/>
    <fo:block chapter="{$pos} of {last()}">
      <xsl:apply-templates/>
    </fo:block>
  </xsl:template>
  <xsl:template match="chapter/title">
    <fo:block font-weight="bold"><xsl:value-of select="."/></fo:block>
  </xsl:template>
  <xsl:template match="section/title" priority="2">
    <fo:block font-style="italic"><xsl:value-of select="."/></fo:block>
  </xsl:template>
  <xsl:template match="para">
    <fo:block>
      <xsl:choose>
        <xsl:when test="preceding-sibling::para">
          <xsl:attribute name="space-before">6pt</xsl:attribute>
        </xsl:when>
        <xsl:otherwise>
          <xsl:attribute name="space-before">0pt</xsl:attribute>
        </xsl:otherwise>
      </xsl:choose>
      <xsl:if test="ancestor::section">
        <xsl:attribute name="start-indent">1em</xsl:attribute>
      </xsl:if>
      <xsl:apply-templates/>
    </fo:block>
  </xsl:template>
  <xsl:template match="em"><fo:inline font-style="italic"><xsl:apply-templates/></fo:inline></xsl:template>
  <xsl:template match="warning[@level='high']">
    <fo:block color="red">WARNING: <xsl:value-of select="."/></fo:block>
  </xsl:template>
  <xsl:template match="figure">
    <fo:block id="{@id}" text="{caption}" nth="{count(preceding::figure) + 1}" same="{generate-id(.) = generate-id(self::figure)}"/>
  </xsl:template>
  <xsl:template match="appendix">
    <fo:list-block>
      <xsl:for-each select="part[number(@price) &gt; 3]">
        <fo:list-item n="{position()}" last="{last()}">
          <xsl:value-of select="concat(@no, ': ', ., ' @ ', @price)"/>
        </fo:list-item>
      </xsl:for-each>
    </fo:list-block>
    <fo:block><xsl:value-of select="part[2]/@no"/>|<xsl:value-of select="part[last()]"/>|<xsl:value-of select="(part/@no)[3]"/>|<xsl:value-of select="part[@price = '3.25'][2]"/></fo:block>
  </xsl:template>
  <xsl:template match="chapter" mode="toc">
    <fo:block toc="{@id}"><xsl:value-of select="title"/> .. <xsl:value-of select="count(.//para)"/></fo:block>
  </xsl:template>
  <xsl:template match="comment()"><xsl:comment>copied:<xsl:value-of select="."/></xsl:comment></xsl:template>
</xsl:stylesheet>
