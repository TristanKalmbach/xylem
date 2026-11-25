<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE xsl:stylesheet [
<!ENTITY U "ABC">
<!ENTITY L "abc">
<!ENTITY U2L " '&U;' , '&L;' ">
<!ENTITY sep "&#45;&#45;">
<!ENTITY greeting "<g>hello &U;</g>">
]>
<!-- Behaviours found by comparing the engine with the reference publishing
     service over many stylesheets. Each template below covers one. -->
<xsl:stylesheet version="2.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
    xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:fo="http://www.w3.org/1999/XSL/Format"
    xmlns:f="urn:x-functions" exclude-result-prefixes="xs f">

  <!-- Patterns match parentless nodes built by a function. -->
  <xsl:function name="f:is-html" as="xs:boolean" version="1.0">
    <xsl:param name="m" as="xs:string"/>
    <xsl:choose>
      <xsl:when test="$m = 'final'"><xsl:value-of select="true()"/></xsl:when>
      <xsl:otherwise><xsl:value-of select="false()"/></xsl:otherwise>
    </xsl:choose>
  </xsl:function>
  <xsl:function name="f:lookup" as="item()*" version="1.0">
    <xsl:param name="name" as="xs:string"/>
    <xsl:choose>
      <xsl:when test="$name = 'known'"><xsl:sequence select="'found'"/></xsl:when>
      <xsl:when test="f:is-html('final')">
        <xsl:call-template name="missing">
          <xsl:with-param name="v" select="$name" as="xs:string"/>
        </xsl:call-template>
      </xsl:when>
    </xsl:choose>
  </xsl:function>
  <xsl:template name="missing">
    <xsl:param name="v"/>
    <fo:inline color="red" font-style="italic">MISSING [<xsl:value-of select="$v"/>]</fo:inline>
    <fo:block a="1"><fo:inline>nested</fo:inline></fo:block>
  </xsl:template>
  <xsl:template match="fo:inline | fo:block" mode="tr">
    <xsl:copy><xsl:apply-templates select="node() | @*" mode="tr"/></xsl:copy>
  </xsl:template>
  <xsl:template match="@*" mode="tr"><xsl:copy/></xsl:template>
  <xsl:template match="fo:block[@a = '1']" mode="pred">first-block</xsl:template>
  <xsl:template match="fo:inline[1]" mode="pred">pos-1</xsl:template>
  <xsl:template match="@color" mode="pred">color</xsl:template>
  <xsl:template match="*" mode="pred">other</xsl:template>
  <xsl:template match="fo:inline" mode="deep"><xsl:text>ancestor-required</xsl:text></xsl:template>
  <xsl:template match="fo:block/fo:inline" mode="deep">child-of-block</xsl:template>

  <xsl:template match="/">
    <parity>
      <parentless>
        <applied><xsl:apply-templates select="f:lookup('unknown')" mode="tr"/></applied>
        <found><xsl:value-of select="f:lookup('known')"/></found>
        <pred><xsl:apply-templates select="f:lookup('unknown')" mode="pred"/></pred>
        <attr><xsl:apply-templates select="f:lookup('unknown')/@color" mode="pred"/></attr>
        <deep><xsl:apply-templates select="f:lookup('unknown')/descendant-or-self::fo:inline" mode="deep"/></deep>
      </parentless>
      <!-- Comments go before whitespace stripping: text around one merges. -->
      <comments>
        <!-- c -->
        x
        <!-- d --><?pi data?>
        <y/>
        <z>   <!-- only whitespace -->   </z>
      </comments>
      <entities>
        <attr><xsl:value-of select="translate('xAyBzC', &U2L;)"/></attr>
        <avt v="{concat('&U;', '&sep;')}"/>
        <content>&L;&sep;&greeting;</content>
      </entities>
    </parity>
  </xsl:template>
</xsl:stylesheet>
