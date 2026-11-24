<xsl:stylesheet version="2.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
    xmlns:xs="http://www.w3.org/2001/XMLSchema" exclude-result-prefixes="xs">
  <xsl:strip-space elements="*"/>
  <xsl:key name="fig" match="figure" use="@id"/>
  <xsl:key name="price" match="part" use="@price"/>
  <xsl:variable name="fmt" select="'(a)'"/>
  <xsl:template match="/">
    <n>
      <xsl:apply-templates select="//chapter | //section | //figure"/>
      <keys>
        <xsl:value-of select="key('fig', 'f2')/caption, count(key('price', '3.25')), count(key('fig', ('f1', 'f3', 'zz')))"/>
      </keys>
      <sorted>
        <xsl:for-each select="//part">
          <xsl:sort select="@price" data-type="number" order="descending"/>
          <xsl:sort select="@no"/>
          <xsl:value-of select="@no"/>
          <xsl:text>;</xsl:text>
        </xsl:for-each>
      </sorted>
      <bytext>
        <xsl:apply-templates select="//part" mode="sorted">
          <xsl:sort select="."/>
        </xsl:apply-templates>
      </bytext>
      <values><xsl:number value="4" format="A."/>|<xsl:number value="12" format="{$fmt}"/>|<xsl:number value="1999" format="I"/>|<xsl:number value="3" format="01"/>|<xsl:number value="(2, 7)" format="1.1"/></values>
    </n>
  </xsl:template>
  <xsl:template match="chapter">
    <c><xsl:number format="1."/></c>
  </xsl:template>
  <xsl:template match="section">
    <s><xsl:number/>/<xsl:number level="multiple" count="chapter | section" format="1.1"/>/<xsl:number level="any" count="section" format="i"/></s>
  </xsl:template>
  <xsl:template match="figure">
    <f id="{@id}" any="{count(preceding::figure) + 1}"><xsl:number level="any" from="chapter"/>-<xsl:number level="any"/>-<xsl:number count="figure" from="section" level="any" format="a"/></f>
  </xsl:template>
  <xsl:template match="part" mode="sorted"><xsl:value-of select="."/>,</xsl:template>
</xsl:stylesheet>
