//! One compiled stylesheet runs transformations on many threads at once.

use std::collections::HashMap;
use xylem_xslt_engine::{Stylesheet, transform_bytes};

#[test]
fn eight_threads_share_one_stylesheet() {
    let dir = std::env::temp_dir().join(format!("xylem-engine-threads-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let xsl = dir.join("s.xsl");
    std::fs::write(
        &xsl,
        r#"<xsl:stylesheet version="2.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
  <xsl:param name="n"/>
  <xsl:key name="k" match="a" use="@v"/>
  <xsl:variable name="g"><x/><x/></xsl:variable>
  <xsl:template match="/">
    <r n="{$n}" k="{count(key('k', '1'))}" g="{count($g/x)}" id="{generate-id(/*)}">
      <xsl:apply-templates select="//a"/>
    </r>
  </xsl:template>
  <xsl:template match="a"><xsl:number level="any"/>,</xsl:template>
</xsl:stylesheet>"#,
    )
    .unwrap();
    let ss = Stylesheet::compile(&xsl).unwrap();
    std::thread::scope(|scope| {
        for t in 0..8usize {
            let ss = &ss;
            scope.spawn(move || {
                for k in 0..25usize {
                    let n = t + k;
                    let input = format!("<d>{}</d>", r#"<a v="1"/>"#.repeat(n));
                    let params = HashMap::from([("n".to_string(), format!("{t}-{k}"))]);
                    let out = transform_bytes(ss, input.as_bytes(), &params, false).unwrap();
                    let numbers: String = (1..=n).map(|i| format!("{i},")).collect();
                    let attrs = format!(r#"<r n="{t}-{k}" k="{n}" g="2" id="d1e1""#);
                    let want = if n == 0 {
                        format!("{attrs} />")
                    } else {
                        format!("{attrs}>{numbers}</r>")
                    };
                    assert_eq!(out.text, want);
                }
            });
        }
    });
    let _ = std::fs::remove_dir_all(&dir);
}
