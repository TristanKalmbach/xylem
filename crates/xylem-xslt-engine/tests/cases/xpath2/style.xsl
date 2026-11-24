<xsl:stylesheet version="2.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
    xmlns:xs="http://www.w3.org/2001/XMLSchema" exclude-result-prefixes="xs">
  <xsl:template match="/">
    <out>
      <seq><xsl:value-of select="//chapter/@id"/></seq>
      <sep><xsl:value-of select="//part/@no" separator=", "/></sep>
      <arith><xsl:value-of select="1 div 3, 2.5 * 2, 7 idiv 2, 7 mod 3, 1e3, 0.1 + 0.2, 10 div 4, 1 div 0e0, -0.0e0, 1000000.0e0, 2 * 0.5, 3 - 5, 0.000001e0, 123456789012e0"/></arith>
      <attrsum><xsl:value-of select="sum(//part/@price), sum(//chapter/@n), avg((1, 2, 4))"/></attrsum>
      <untyped><xsl:value-of select="//chapter[@n &gt; 5]/@id, //chapter[@n = 3]/@id, //chapter[@n = '3']/@id"/></untyped>
      <cond><xsl:value-of select="if (count(//para) gt 5) then 'many' else 'few'"/></cond>
      <for><xsl:value-of select="for $c in //chapter return string-length($c/title)"/></for>
      <range><xsl:value-of select="(1 to 5)[. mod 2 = 1]"/></range>
      <quant><xsl:value-of select="some $p in //part satisfies $p/@price &gt; 50, every $p in //part satisfies $p/@price &gt; 50"/></quant>
      <strings><xsl:value-of select="upper-case('abc'), lower-case('XyZ'), substring('12345', 2, 3), substring-before('a-b-c', '-'), substring-after('a-b-c', '-'), translate('hello', 'el', 'EL'), normalize-space('  a   b  '), string-join(('x','y','z'), '+')" separator="|"/></strings>
      <regex><xsl:value-of select="replace('2026-11-24', '(\d+)-(\d+)-(\d+)', '$3.$2.$1'), matches('P-11', '^P-\d+$'), tokenize('a, b,c', ',\s*')" separator="|"/></regex>
      <distinct><xsl:value-of select="distinct-values(//part/@price)"/></distinct>
      <starts><xsl:value-of select="count(//part[starts-with(@no, 'P-2')]), count(//part[ends-with(., 'er')]), count(//*[contains(name(), 'ion')])"/></starts>
      <casts><xsl:value-of select="xs:integer('42') + 1, xs:decimal('1.50'), xs:string(12), xs:boolean('true'), number('x'), round(2.5), round(-2.5), floor(2.7), ceiling(2.1), abs(-3)"/></casts>
      <cmp><xsl:value-of select="'abc' lt 'abd', 2 eq 2.0, (1, 2) = (2, 3), (1, 2) != (1, 2), 1 &lt; 2"/></cmp>
      <nodes><xsl:value-of select="(//title)[1] is /book/title, count(//para except //section/para), count(//para intersect //chapter[1]//para)"/></nodes>
      <axes><xsl:value-of select="//figure[@id='f3']/ancestor::*[1]/title, //figure[@id='f3']/preceding::figure[1]/@id, //section[1]/following-sibling::*[1]/name(), name(//qty/..)"/></axes>
      <empty><xsl:value-of select="empty(//missing), exists(//part), count(()), string(())"/></empty>
      <minmax><xsl:value-of select="max((3, 9, 2)), min(//chapter/@n), max(('b','a'))"/></minmax>
      <fmt><xsl:value-of select="format-number(1234.5, '#,##0.00'), format-number(0.5, '0.0')"/></fmt>
    </out>
  </xsl:template>
</xsl:stylesheet>
