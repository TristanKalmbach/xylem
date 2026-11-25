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
  <xsl:include href="sub/inc.xsl"/>

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
  <xsl:template match="attribute()" mode="attrtest">[<xsl:value-of select="name()"/>]</xsl:template>
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
      <!-- URIs: the source's base is the stylesheet's folder; resolved
           file URIs drop the empty authority. -->
      <uris>
        <sbu><xsl:value-of select="ends-with(static-base-uri(), '/cases/parity/style.xsl') and starts-with(static-base-uri(), 'file:///')"/></sbu>
        <bu><xsl:value-of select="ends-with(base-uri(), '/cases/parity')"/></bu>
        <du><xsl:value-of select="document-uri(/) = base-uri(/)"/></du>
        <tmp><xsl:variable name="v"><e/></xsl:variable><xsl:value-of select="base-uri($v/e) = static-base-uri(), count(document-uri($v))"/></tmp>
        <r1><xsl:value-of select="substring-after(resolve-uri('../a b.png', static-base-uri()), '/cases/')"/></r1>
        <r2><xsl:value-of select="starts-with(resolve-uri('x.png'), 'file:/') and not(starts-with(resolve-uri('x.png'), 'file://'))"/></r2>
        <r3><xsl:value-of select="resolve-uri('../../c?q=1#f', 'http://e.com/a/b/c'), resolve-uri('', 'http://e.com/a'), resolve-uri('/x', 'http://e.com/a/b')"/></r3>
        <r4><xsl:value-of select="count(resolve-uri(())), base-uri() instance of xs:anyURI"/></r4>
      </uris>
      <!-- A CR is written as it is; CR LF in an attribute is one space. -->
      <lines a="s1
s2"><xsl:text>t1&#13;t2&#10;t3</xsl:text></lines>
      <!-- xsl:number when no node matches `from`: any and multiple count
           as if there were no `from`; single gives nothing. -->
      <numbers>
        <xsl:variable name="d"><doc><x/><s><x/><x><y/><y/></x></s><x><y/></x></doc></xsl:variable>
        <xsl:for-each select="$d//x | $d//y">
          <n><xsl:number count="x|y" from="nomatch" level="any"/>|<xsl:number count="x" from="nomatch" level="single"/>|<xsl:number count="x|y" from="nomatch" level="multiple" format="1.1"/>|<xsl:number count="x|y" from="s" level="multiple" format="1.1"/>|<xsl:number count="x" from="s" level="any"/></n>
        </xsl:for-each>
      </numbers>
      <modules>
        <xsl:call-template name="module-relative"/>
        <top><xsl:value-of select="document('data.xml')/data/v"/></top>
      </modules>
      <!-- Simple content merges adjacent text nodes before separating
           items, in attribute value templates too. -->
      <simple>
        <xsl:variable name="t"><a>x</a><a>y</a><a>z</a></xsl:variable>
        <v><xsl:value-of select="$t/a/text()"/></v>
        <w><xsl:value-of select="$t/a/text(), 1, 2, $t/a[1]/text()" separator="|"/></w>
        <x a="{$t/a/text()}"><xsl:attribute name="b" select="$t/a/text()"/></x>
      </simple>
      <!-- xsl:namespace and lang(). -->
      <ns><xsl:element name="e"><xsl:namespace name="p" select="'urn:p'"/><xsl:namespace name="q">urn:<xsl:value-of select="'q'"/></xsl:namespace><xsl:attribute name="p:a" namespace="urn:p">1</xsl:attribute></xsl:element></ns>
      <lang>
        <xsl:variable name="l"><d xml:lang="en-US"><p/><q xml:lang="de"/></d></xsl:variable>
        <xsl:value-of select="lang('en', $l/d/p), lang('EN-us', $l/d), lang('en', $l/d/q), lang('fr', $l/d/p)"/>
      </lang>
      <!-- Text sorts by culture (1.0 always, 2.0 with case-order). -->
      <sorts><xsl:variable name="w"><w>Vac</w><w>VHMC</w><w>apple</w><w>Apple</w><w>APPLE</w><w>b</w><w>B</w><w>co-op</w><w>coop</w><w>café</w><w>cafe</w><w>a1</w><w>a10</w><w>a2</w><w>_x</w><w>Zebra</w><w>zebra</w><w>élan</w><w>elan</w><w>Élan</w><w>Ab</w><w>aB</w><w>ab</w><w>AB</w><w>1</w><w>10</w><w>9</w><w>-a</w><w>a-</w><w>résumé</w><w>resume</w><w>Resume</w><w>ß</w><w>ss</w></xsl:variable><a><xsl:for-each select="$w/w"><xsl:sort select="."/><xsl:value-of select="."/>|</xsl:for-each></a><u><xsl:for-each select="$w/w"><xsl:sort select="." case-order="upper-first"/><xsl:value-of select="."/>|</xsl:for-each></u></sorts>
      <!-- attribute() without an axis selects attributes, in expressions
           and in patterns. -->
      <attrtest>
        <xsl:variable name="e"><e a="1" b="2">t<f/></e></xsl:variable>
        <xsl:apply-templates select="$e/e/(text() | attribute())" mode="attrtest"/>|<xsl:value-of select="count($e/e/attribute()), count($e/e/attribute(b)), count($e/e/child::attribute())"/>
      </attrtest>
      <entities>
        <attr><xsl:value-of select="translate('xAyBzC', &U2L;)"/></attr>
        <avt v="{concat('&U;', '&sep;')}"/>
        <content>&L;&sep;&greeting;</content>
      </entities>
    </parity>
  </xsl:template>
</xsl:stylesheet>
