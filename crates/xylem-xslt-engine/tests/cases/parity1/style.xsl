<?xml version="1.0" encoding="UTF-8"?>
<!-- XSLT 1.0 behaviours of the reference publishing service, found by
     comparing the engine with it over many stylesheets. -->
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
    xmlns:fo="http://www.w3.org/1999/XSL/Format" xmlns:x="urn:x-ext" xmlns:my="urn:x-words" exclude-result-prefixes="my">
  <my:words xmlns:my="urn:x-words"><w>Vac</w><w>VHF</w><w>apple</w><w>Apple</w><w>APPLE</w><w>b</w><w>B</w><w>co-op</w><w>coop</w><w>café</w><w>cafe</w><w>a1</w><w>a10</w><w>a2</w><w>_x</w><w>Zebra</w><w>zebra</w><w>élan</w><w>elan</w><w>Élan</w><w>Ab</w><w>aB</w><w>ab</w><w>AB</w><w>1</w><w>10</w><w>9</w><w>-a</w><w>a-</w><w>résumé</w><w>resume</w><w>Resume</w><w>ß</w><w>ss</w></my:words>
  <xsl:template match="/">
    <fo:root font-size="10pt">
      <!-- Namespace declarations follow the attributes. -->
      <fo:block color="red" x:flag="1">
        <xsl:attribute name="id">b1</xsl:attribute>
        <inner xmlns="urn:x-inner" a="1"/>
      </fo:block>
      <!-- Zero-length text from value-of or copy-of gives a full end tag. -->
      <empty><a><xsl:value-of select="''"/></a><b/><c><xsl:text></xsl:text></c><f><xsl:copy-of select="''"/></f><g x="1"><xsl:value-of select="/nothing"/></g><k><xsl:attribute name="y">2</xsl:attribute></k></empty>
      <!-- Line ends in text are written as CR LF; attribute values keep
           their character references. -->
      <lines a="s1
s2" c="&#10;|&#13;">lit1
lit2<xsl:text>t1&#10;t2&#13;t3&#13;&#10;t4</xsl:text></lines>
      <!-- xsl:number with no node matching `from` gives nothing. -->
      <numbers>
        <xsl:for-each select="//*">
          <n><xsl:number count="*" from="nomatch" level="any"/>|<xsl:number count="*" from="nomatch" level="multiple"/></n>
        </xsl:for-each>
      </numbers>
      <!-- Text sorts by culture (1.0 always, 2.0 with case-order). -->
      <sorts><a><xsl:for-each select="document('')/*/my:words/w"><xsl:sort select="."/><xsl:value-of select="."/>|</xsl:for-each></a><u><xsl:for-each select="document('')/*/my:words/w"><xsl:sort select="." case-order="upper-first"/><xsl:value-of select="."/>|</xsl:for-each></u></sorts>
    </fo:root>
  </xsl:template>
</xsl:stylesheet>
