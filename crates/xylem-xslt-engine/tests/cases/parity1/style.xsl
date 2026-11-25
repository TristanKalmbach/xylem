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
    </fo:root>
  </xsl:template>
</xsl:stylesheet>
