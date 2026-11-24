<xsl:stylesheet version="2.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
<xsl:import href="m3.xsl"/><xsl:include href="m1.xsl"/><xsl:include href="m2.xsl"/>
<xsl:variable name="g"><x/></xsl:variable>
<xsl:variable name="d" select="document('m1.xsl')"/>
<xsl:template match="/"><o src="{generate-id(/)}"><xsl:variable name="l"><y/></xsl:variable><t l="{generate-id($l)}" g="{generate-id($g)}" d="{generate-id($d)}" src2="{generate-id(/*)}"/></o></xsl:template></xsl:stylesheet>
