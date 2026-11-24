<xsl:stylesheet version="2.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
    xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:my="urn:example:functions"
    exclude-result-prefixes="xs my">
  <xsl:strip-space elements="*"/>
  <xsl:param name="mode" select="'draft'"/>
  <xsl:param name="pageCount" as="xs:integer" select="0"/>
  <xsl:attribute-set name="base">
    <xsl:attribute name="font-size">10pt</xsl:attribute>
    <xsl:attribute name="font-family">sans</xsl:attribute>
  </xsl:attribute-set>
  <xsl:attribute-set name="heading" use-attribute-sets="base">
    <xsl:attribute name="font-size">14pt</xsl:attribute>
  </xsl:attribute-set>
  <xsl:function name="my:label" as="xs:string">
    <xsl:param name="n" as="xs:integer"/>
    <xsl:param name="prefix" as="xs:string"/>
    <xsl:sequence select="concat($prefix, ' ', $n)"/>
  </xsl:function>
  <xsl:function name="my:is-big" as="xs:boolean">
    <xsl:param name="p"/>
    <xsl:sequence select="xs:decimal($p/@price) ge 10"/>
  </xsl:function>
  <xsl:template match="/">
    <doc mode="{$mode}" pages="{$pageCount + 1}">
      <xsl:apply-templates select="book/chapter">
        <xsl:with-param name="depth" select="1" tunnel="yes"/>
        <xsl:with-param name="plain" select="'p'"/>
      </xsl:apply-templates>
      <xsl:apply-templates select="book/appendix" mode="parts"/>
      <xsl:call-template name="named">
        <xsl:with-param name="x" select="5"/>
      </xsl:call-template>
      <xsl:call-template name="named"/>
    </doc>
  </xsl:template>
  <xsl:template match="chapter">
    <xsl:param name="plain" select="'default'"/>
    <xsl:element name="ch" use-attribute-sets="heading">
      <xsl:attribute name="plain" select="$plain"/>
      <xsl:attribute name="label" select="my:label(xs:integer(@n), 'Chapter')"/>
      <xsl:apply-templates select="section"/>
    </xsl:element>
  </xsl:template>
  <xsl:template match="section">
    <xsl:param name="depth" tunnel="yes" as="xs:integer"/>
    <xsl:param name="plain" select="'unset'"/>
    <sec depth="{$depth}" plain="{$plain}" xsl:use-attribute-sets="base">
      <xsl:apply-templates select="title" mode="other"/>
    </sec>
  </xsl:template>
  <xsl:template match="title" mode="#all">
    <t><xsl:value-of select="."/></t>
  </xsl:template>
  <xsl:template match="appendix" mode="parts">
    <xsl:variable name="big" as="element()*" select="part[my:is-big(.)]"/>
    <xsl:variable name="names" as="xs:string*">
      <xsl:for-each select="part">
        <xsl:sequence select="string(.)"/>
      </xsl:for-each>
    </xsl:variable>
    <xsl:variable name="tree">
      <wrap><xsl:copy-of select="part[1]"/></wrap>
    </xsl:variable>
    <parts big="{count($big)}" names="{$names}" n="{count($names)}">
      <xsl:copy-of select="part[last()]/@no"/>
      <xsl:copy-of select="$tree/wrap/part"/>
      <xsl:apply-templates select="part" mode="#current"/>
      <xsl:value-of select="$tree"/>
    </parts>
  </xsl:template>
  <xsl:template match="part" mode="parts">
    <xsl:copy>
      <xsl:attribute name="big" select="my:is-big(.)"/>
      <xsl:value-of select="."/>
    </xsl:copy>
  </xsl:template>
  <xsl:template match="part[@no = 'P-2']" mode="parts" priority="1">
    <bolt/>
  </xsl:template>
  <xsl:template name="named">
    <xsl:param name="x" select="1"/>
    <xsl:variable name="y" as="xs:integer">
      <xsl:value-of select="$x * 2"/>
    </xsl:variable>
    <named x="{$x}" y="{$y + 1}"><xsl:comment>n<xsl:value-of select="$y"/></xsl:comment></named>
  </xsl:template>
</xsl:stylesheet>
