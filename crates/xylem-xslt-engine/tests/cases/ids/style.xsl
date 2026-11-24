<xsl:stylesheet version="2.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform"><xsl:strip-space elements="*"/>
<xsl:template match="/"><out><xsl:for-each select="//node()"><n k="{name()}{if (self::text()) then concat('#', string-length(.)) else ''}" id="{generate-id()}"/></xsl:for-each></out></xsl:template></xsl:stylesheet>
