<?xml version="1.0" encoding="UTF-8"?>
<!-- XSLT 1.0 behaviours of the reference publishing service, found by
     comparing the engine with it over many stylesheets. -->
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
    xmlns:fo="http://www.w3.org/1999/XSL/Format" xmlns:x="urn:x-ext">
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
    </fo:root>
  </xsl:template>
</xsl:stylesheet>
