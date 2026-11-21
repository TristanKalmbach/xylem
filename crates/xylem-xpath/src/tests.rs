use super::*;

const SAMPLE: &str = r#"<?xml version="1.0"?>
<!-- top -->
<doc xml:lang="en-US" xmlns:s="urn:s">
  <?Ref_Label first?>
  <chapter id="c1" n="1">
    <title>One</title>
    <para id="x">Hello <b>big</b> world</para>
    <para id="y">Second</para>
    <para>Third</para>
    <?Ref_Label second?>
    <!-- note -->
  </chapter>
  <chapter id="c2" n="2" xml:lang="fr"><title>Two</title><s:item>a</s:item><s:item>b</s:item></chapter>
  <nums><n>1</n><n>2</n><n>3</n><n>10</n></nums>
</doc>"#;

fn doc() -> Document {
    Document::from_str(SAMPLE).expect("sample parses")
}

fn sel(d: &Document, expr: &str) -> Vec<XNode> {
    select(d, d.root(), expr).unwrap_or_else(|e| panic!("{expr}: {e}"))
}

fn num(d: &Document, expr: &str) -> f64 {
    evaluate(d, d.root(), expr).unwrap().to_number(d)
}

fn text(d: &Document, expr: &str) -> String {
    evaluate(d, d.root(), expr).unwrap().to_string_value(d)
}

fn boolean(d: &Document, expr: &str) -> bool {
    evaluate(d, d.root(), expr).unwrap().to_boolean()
}

fn names(d: &Document, nodes: &[XNode]) -> Vec<String> {
    nodes
        .iter()
        .map(|&n| match n {
            XNode::Node(id) => d.name(id).unwrap_or("?").to_string(),
            XNode::Attr(id, i) => format!("@{}", d.element(id).unwrap().attrs[i].name),
        })
        .collect()
}

#[test]
fn xmax_style_queries() {
    let d = doc();
    let pis = sel(&d, "//processing-instruction('Ref_Label')");
    assert_eq!(pis.len(), 2);
    assert_eq!(string_value(&d, pis[0]), "first");
    assert_eq!(string_value(&d, pis[1]), "second");

    let paras = sel(&d, "descendant::para[@id='x']");
    assert_eq!(paras.len(), 1);
    // From a context node, descendant:: is relative.
    let chapter = sel(&d, "/doc/chapter[1]")[0].node().unwrap();
    let from_chapter = select(&d, chapter, "descendant::para[@id='y']").unwrap();
    assert_eq!(from_chapter.len(), 1);
    assert!(select(&d, chapter, "descendant::title[.='Two']").unwrap().is_empty());
}

#[test]
fn abbreviations_and_axes() {
    let d = doc();
    assert_eq!(sel(&d, "//para").len(), 3);
    assert_eq!(sel(&d, "/doc/chapter/para[last()]").len(), 1);
    assert_eq!(string_value(&d, sel(&d, "/doc/chapter/para[last()]")[0]), "Third");
    assert_eq!(string_value(&d, sel(&d, "//para[2]")[0]), "Second");
    assert_eq!(names(&d, &sel(&d, "//b/..")), ["para"]);
    assert_eq!(names(&d, &sel(&d, "//b/ancestor::*")), ["doc", "chapter", "para"]);
    assert_eq!(names(&d, &sel(&d, "//b/ancestor-or-self::*")), ["doc", "chapter", "para", "b"]);
    assert_eq!(names(&d, &sel(&d, "/doc/chapter[1]/title/following-sibling::para")), ["para", "para", "para"]);
    assert_eq!(names(&d, &sel(&d, "//para[@id='y']/preceding-sibling::*")), ["title", "para"]);
    assert_eq!(string_value(&d, sel(&d, "//para[@id='y']/preceding-sibling::*[1]")[0]), "Hello big world");
    assert_eq!(names(&d, &sel(&d, "//title[.='One']/following::title")), ["title"]);
    assert_eq!(names(&d, &sel(&d, "//title[.='Two']/preceding::title")), ["title"]);
    assert_eq!(sel(&d, "//nums/n/self::n").len(), 4);
    assert_eq!(sel(&d, "//n[position() mod 2 = 0]").len(), 2);
    assert_eq!(sel(&d, "/doc/descendant-or-self::*").len(), 16);
    assert_eq!(sel(&d, "/descendant::node()/self::chapter").len(), 2);
}

#[test]
fn preceding_numbers_in_reverse_order() {
    let d = doc();
    // preceding::*[1] is the nearest preceding element in reverse document order.
    let n = sel(&d, "//n[.='10']/preceding::n[1]");
    assert_eq!(string_value(&d, n[0]), "3");
    let all = sel(&d, "//n[.='10']/preceding::n");
    assert_eq!(all.len(), 3);
    assert_eq!(string_value(&d, all[0]), "1");
}

#[test]
fn attribute_nodes() {
    let d = doc();
    let ids = sel(&d, "//@id");
    assert_eq!(ids.len(), 4);
    assert!(ids.iter().all(|n| n.is_attr()));
    assert_eq!(string_value(&d, ids[0]), "c1");
    assert_eq!(sel(&d, "//chapter/@*").len(), 5);
    assert_eq!(sel(&d, "//@xmlns:s").len(), 0, "namespace declarations are not attributes");
    assert_eq!(text(&d, "//para[@id='y']/@id"), "y");
    assert_eq!(names(&d, &sel(&d, "//para/@id/..")), ["para", "para"]);
    assert_eq!(sel(&d, "//@id/parent::chapter").len(), 2);
    assert_eq!(sel(&d, "//@id/ancestor::doc").len(), 1);
    assert_eq!(sel(&d, "//chapter[@n > 1]").len(), 1);
    assert_eq!(sel(&d, "//chapter[@n = 1 or @n = 2]").len(), 2);
}

#[test]
fn node_tests() {
    let d = doc();
    assert_eq!(sel(&d, "//comment()").len(), 2);
    assert_eq!(sel(&d, "/comment()").len(), 1);
    assert_eq!(sel(&d, "//para/text()").len(), 4);
    assert_eq!(sel(&d, "//processing-instruction()").len(), 2);
    assert_eq!(sel(&d, "//processing-instruction('nope')").len(), 0);
    assert_eq!(sel(&d, "//s:item").len(), 2);
    assert_eq!(sel(&d, "//s:*").len(), 2);
    assert_eq!(sel(&d, "//item").len(), 0);
    assert_eq!(sel(&d, "/doc/*").len(), 3);
    assert_eq!(num(&d, "count(/doc/node())"), num(&d, "count(/doc/*) + count(/doc/text()) + count(/doc/processing-instruction())"));
}

#[test]
fn namespace_context() {
    let d = Document::from_str(r#"<r xmlns:a="urn:x"><a:i/><b:i xmlns:b="urn:x"/><a:j/></r>"#).unwrap();
    let mut ctx = Context::new();
    ctx.set_namespace("q", "urn:x");
    let xp = compile("//q:i").unwrap();
    let hits = xp.select_with(&d, XNode::Node(d.root()), &ctx).unwrap();
    assert_eq!(hits.len(), 2);
    let wild = compile("//q:*").unwrap().select_with(&d, XNode::Node(d.root()), &ctx).unwrap();
    assert_eq!(wild.len(), 3);
    // Without a mapping, prefixes are compared literally.
    assert_eq!(sel(&d, "//a:i").len(), 1);
    assert_eq!(text(&d, "namespace-uri(/r/*[1])"), "urn:x");
    assert_eq!(text(&d, "local-name(/r/*[2])"), "i");
    assert_eq!(text(&d, "name(/r/*[2])"), "b:i");
}

#[test]
fn unions_are_ordered_and_deduplicated() {
    let d = doc();
    let hits = sel(&d, "//title | //para | //title | //n");
    assert_eq!(names(&d, &hits), ["title", "para", "para", "para", "title", "n", "n", "n", "n"]);
    let mut order: Vec<_> = hits.clone();
    order.sort_by(|a, b| d.cmp_order(a.owner(), b.owner()));
    assert_eq!(order, hits);
    assert_eq!(sel(&d, "//para | //para").len(), 3);
}

#[test]
fn arithmetic_and_comparison() {
    let d = doc();
    assert_eq!(num(&d, "1 + 2 * 3"), 7.0);
    assert_eq!(num(&d, "(1 + 2) * 3"), 9.0);
    assert_eq!(num(&d, "7 div 2"), 3.5);
    assert_eq!(num(&d, "7 mod 3"), 1.0);
    assert_eq!(num(&d, "-7 mod 3"), -1.0);
    assert_eq!(num(&d, "5 - -2"), 7.0);
    assert!(num(&d, "1 div 0").is_infinite());
    assert!(num(&d, "0 div 0").is_nan());
    assert!(boolean(&d, "1 < 2 and 2 <= 2 and 3 > 2 and 3 >= 3 and 1 != 2"));
    assert!(boolean(&d, "'a' = 'a'"));
    assert!(boolean(&d, "'1' = 1"));
    assert!(boolean(&d, "true() = 'x'"));
    assert!(!boolean(&d, "1 = 'x'"));
    assert!(boolean(&d, "false() or 1"));
    assert_eq!(text(&d, "2 * 3"), "6");
    assert_eq!(text(&d, "10 div 4"), "2.5");
    assert_eq!(text(&d, "1 div 0"), "Infinity");
}

#[test]
fn node_set_comparison_semantics() {
    let d = doc();
    assert!(boolean(&d, "//n = 3"));
    assert!(boolean(&d, "//n != 3"), "some n differs from 3");
    assert!(boolean(&d, "//n > 9"));
    assert!(!boolean(&d, "//n > 10"));
    assert!(boolean(&d, "//n >= 10"));
    assert!(boolean(&d, "5 > //n"));
    assert!(!boolean(&d, "0 > //n"));
    assert!(boolean(&d, "//title = 'One'"));
    assert!(boolean(&d, "'Two' = //title"));
    assert!(boolean(&d, "//title = //chapter/title"));
    assert!(!boolean(&d, "//nope = 'x'"));
    assert!(!boolean(&d, "//nope != 'x'"), "empty node-sets never compare true");
    assert!(boolean(&d, "//n = true()"));
    assert!(boolean(&d, "//nope = false()"));
    assert!(boolean(&d, "//n[1] < //n[2]"));
    assert!(boolean(&d, "//chapter/@n = 2"));
}

#[test]
fn string_functions() {
    let d = doc();
    assert_eq!(text(&d, "concat('a', 'b', 'c')"), "abc");
    assert!(boolean(&d, "starts-with('hello', 'he')"));
    assert!(!boolean(&d, "starts-with('hello', 'lo')"));
    assert!(boolean(&d, "contains('hello', 'ell')"));
    assert_eq!(text(&d, "substring-before('1999/04/01', '/')"), "1999");
    assert_eq!(text(&d, "substring-after('1999/04/01', '/')"), "04/01");
    assert_eq!(text(&d, "substring-after('abc', 'x')"), "");
    assert_eq!(text(&d, "substring('12345', 2, 3)"), "234");
    assert_eq!(text(&d, "substring('12345', 2)"), "2345");
    assert_eq!(text(&d, "substring('12345', 1.5, 2.6)"), "234");
    assert_eq!(text(&d, "substring('12345', 0, 3)"), "12");
    assert_eq!(text(&d, "substring('12345', 0 div 0, 3)"), "");
    assert_eq!(text(&d, "substring('12345', -42, 1 div 0)"), "12345");
    assert_eq!(num(&d, "string-length('héllo')"), 5.0);
    assert_eq!(num(&d, "string-length(//title[1])"), 3.0);
    assert_eq!(text(&d, "normalize-space('  a   b \t c ')"), "a b c");
    assert_eq!(text(&d, "translate('bar', 'abc', 'ABC')"), "BAr");
    assert_eq!(text(&d, "translate('--aaa--', 'abc-', 'ABC')"), "AAA");
    assert_eq!(text(&d, "string(//para[1])"), "Hello big world");
    assert_eq!(text(&d, "string(1.50)"), "1.5");
    assert_eq!(text(&d, "string(true())"), "true");
    assert_eq!(text(&d, "string(0 div 0)"), "NaN");
    assert_eq!(text(&d, "string(-1 div 0)"), "-Infinity");
}

#[test]
fn number_and_boolean_functions() {
    let d = doc();
    assert_eq!(num(&d, "count(//para)"), 3.0);
    assert_eq!(num(&d, "sum(//n)"), 16.0);
    assert_eq!(num(&d, "floor(2.7)"), 2.0);
    assert_eq!(num(&d, "floor(-2.5)"), -3.0);
    assert_eq!(num(&d, "ceiling(2.1)"), 3.0);
    assert_eq!(num(&d, "round(2.5)"), 3.0);
    assert_eq!(num(&d, "round(-2.5)"), -2.0);
    assert_eq!(num(&d, "round(2.4)"), 2.0);
    assert!(num(&d, "round(0 div 0)").is_nan());
    assert_eq!(num(&d, "number('  42 ')"), 42.0);
    assert!(num(&d, "number('abc')").is_nan());
    assert_eq!(num(&d, "number(true())"), 1.0);
    assert!(boolean(&d, "boolean(//para)"));
    assert!(!boolean(&d, "boolean(//nope)"));
    assert!(!boolean(&d, "boolean('')"));
    assert!(!boolean(&d, "boolean(0)"));
    assert!(boolean(&d, "not(//nope)"));
    assert!(boolean(&d, "true() and not(false())"));
    assert!(num(&d, "sum(//title)").is_nan());
}

#[test]
fn name_functions_use_context_node() {
    let d = doc();
    let title = sel(&d, "//title")[0].node().unwrap();
    let ev = |expr: &str| compile(expr).unwrap().evaluate(&d, XNode::Node(title)).unwrap();
    assert_eq!(ev("name()").to_string_value(&d), "title");
    assert_eq!(ev("local-name(..)").to_string_value(&d), "chapter");
    assert_eq!(ev("string()").to_string_value(&d), "One");
    assert_eq!(ev("string-length()").to_string_value(&d), "3");
    assert_eq!(ev("name(../@id)").to_string_value(&d), "id");
    assert_eq!(ev("name(/)").to_string_value(&d), "");
    assert_eq!(ev("name(//processing-instruction())").to_string_value(&d), "Ref_Label");
}

#[test]
fn id_and_lang() {
    let d = doc();
    assert_eq!(sel(&d, "id('x')").len(), 1);
    assert_eq!(sel(&d, "id('x c2 nope')").len(), 2);
    assert_eq!(names(&d, &sel(&d, "id('c1')/para/@id")), ["@id", "@id"]);
    assert!(boolean(&d, "/doc/chapter[1][lang('en')]"));
    assert!(boolean(&d, "/doc/chapter[1][lang('en-US')]"));
    assert!(!boolean(&d, "/doc/chapter[1][lang('fr')]"));
    assert!(boolean(&d, "/doc/chapter[2][lang('FR')]"));
    assert!(!boolean(&d, "/doc/chapter[2][lang('en')]"));
}

#[test]
fn predicates_and_positions() {
    let d = doc();
    assert_eq!(string_value(&d, sel(&d, "(//para)[3]")[0]), "Third");
    assert_eq!(string_value(&d, sel(&d, "(//para)[last()]")[0]), "Third");
    assert_eq!(sel(&d, "//chapter/para[1]").len(), 1);
    assert_eq!(sel(&d, "//para[position() > 1]").len(), 2);
    assert_eq!(sel(&d, "//para[@id][2]").len(), 1);
    assert_eq!(string_value(&d, sel(&d, "//para[@id][2]")[0]), "Second");
    assert_eq!(sel(&d, "//para[not(@id)]").len(), 1);
    assert_eq!(sel(&d, "//chapter[para]").len(), 1);
    assert_eq!(sel(&d, "//chapter[count(para) = 3]").len(), 1);
    assert_eq!(sel(&d, "//para[b]").len(), 1);
    assert_eq!(sel(&d, "//para[contains(., 'big')]").len(), 1);
    assert_eq!(sel(&d, "//n[. > 1][. < 10]").len(), 2);
    assert_eq!(sel(&d, "//n[1.5]").len(), 0);
    assert_eq!(sel(&d, "//n[0]").len(), 0);
    assert_eq!(string_value(&d, sel(&d, "//n[last()]")[0]), "10");
}

#[test]
fn double_slash_semantics() {
    let d = doc();
    // `//para[1]` is every para that is the first para child of its parent.
    let d2 = Document::from_str("<r><s><p>1</p><p>2</p></s><s><p>3</p></s></r>").unwrap();
    assert_eq!(select(&d2, d2.root(), "//p[1]").unwrap().len(), 2);
    assert_eq!(select(&d2, d2.root(), "/descendant::p[1]").unwrap().len(), 1);
    assert_eq!(select(&d2, d2.root(), "//s//p").unwrap().len(), 3);
    assert_eq!(select(&d2, d2.root(), "//s/p[last()]").unwrap().len(), 2);
    assert_eq!(select(&d2, d2.root(), "//p/text()").unwrap().len(), 3);
    assert_eq!(sel(&d, ".//para").len(), 3);
    assert_eq!(sel(&d, "/").len(), 1);
}

#[test]
fn variables_and_custom_functions() {
    let d = doc();
    let mut ctx = Context::new();
    ctx.set_variable("id", "y").set_variable("limit", 2.0);
    let paras = sel(&d, "//para");
    ctx.set_variable("paras", paras.clone());
    ctx.register_function("shout", |_, args| {
        let s = args.first().ok_or_else(|| XPathError::custom("shout() needs an argument"))?;
        Ok(Value::String(format!("{}!", match s {
            Value::String(s) => s.clone(),
            _ => String::new(),
        })))
    });
    ctx.register_function("pos", |fc, _| Ok(Value::Number(fc.position as f64)));
    let root = XNode::Node(d.root());
    let run = |e: &str| compile(e).unwrap().evaluate_with(&d, root, &ctx);
    assert_eq!(run("//para[@id=$id]").unwrap().as_node_set().unwrap().len(), 1);
    assert_eq!(run("count($paras)").unwrap(), Value::Number(3.0));
    assert_eq!(run("$limit + 1").unwrap(), Value::Number(3.0));
    assert_eq!(run("shout('hey')").unwrap(), Value::String("hey!".into()));
    assert_eq!(run("count(//n[pos() = 2])").unwrap(), Value::Number(1.0));
    assert_eq!(run("$paras[2]/@id").unwrap().to_string_value(&d), "y");
    assert!(run("$missing").is_err());
}

#[test]
fn nodes_come_back_in_document_order_for_large_documents() {
    let mut xml = String::from("<r>");
    for i in 0..5000 {
        xml.push_str(&format!("<a i='{i}'><b/><b/><c><b/></c></a>"));
    }
    xml.push_str("</r>");
    let d = Document::from_str(&xml).unwrap();
    assert!(d.node_count() > 20_000);
    let hits = sel(&d, "//b | //a | //c");
    assert_eq!(hits.len(), 5000 * 5);
    assert!(hits.windows(2).all(|w| d.cmp_order(w[0].owner(), w[1].owner()).is_lt()));
    assert_eq!(sel(&d, "//a[@i='4999']").len(), 1);
    assert_eq!(sel(&d, "//b/ancestor::a").len(), 5000);
    assert_eq!(sel(&d, "//c/preceding-sibling::b").len(), 10_000);
    assert_eq!(sel(&d, "//a[@i='0']/following-sibling::a").len(), 4999);
    assert_eq!(sel(&d, "//b/parent::*").len(), 10_000);
    assert_eq!(num(&d, "count(//b)"), 15_000.0);
}

#[test]
fn syntax_errors_carry_positions() {
    for (expr, pos) in [("a[", 2), ("a/", 2), ("1 +", 3), ("a b", 2), ("child::", 7), ("@", 1), ("'x", 0), ("$", 1), ("a ! b", 2)] {
        let err = compile(expr).expect_err(expr);
        assert_eq!(err.position(), Some(pos), "{expr}: {err}");
    }
    assert!(compile("bogus::a").is_err());
    assert!(compile("a | ").is_err());
    assert!(compile("foo(").is_err());
    assert!(compile("").is_err());
    assert!(compile(&"(".repeat(1000)).is_err(), "deep nesting is rejected, not a stack overflow");
}

#[test]
fn evaluation_errors() {
    let d = doc();
    assert!(evaluate(&d, d.root(), "nosuchfn()").is_err());
    assert!(evaluate(&d, d.root(), "count(1)").is_err());
    assert!(evaluate(&d, d.root(), "count()").is_err());
    assert!(evaluate(&d, d.root(), "1 | 2").is_err());
    assert!(evaluate(&d, d.root(), "(1)/a").is_err());
    assert!(select(&d, d.root(), "1 + 1").is_err());
    assert!(evaluate(&d, d.root(), "concat('a')").is_err());
    let msg = evaluate(&d, d.root(), "count(1)").unwrap_err().to_string();
    assert!(msg.contains("node-set"), "{msg}");
}

#[test]
fn lexer_disambiguation() {
    let d = doc();
    // `*` as multiplication vs wildcard, and operator names used as element names.
    assert_eq!(num(&d, "2 * 3 * 4"), 24.0);
    assert_eq!(sel(&d, "/doc/*").len(), 3);
    assert_eq!(num(&d, "count(/doc/*) * 2"), 6.0);
    let e = Document::from_str("<div><and/><or/><mod/><div/></div>").unwrap();
    assert_eq!(select(&e, e.root(), "/div/and | /div/or | /div/mod | /div/div").unwrap().len(), 4);
    assert_eq!(select(&e, e.root(), "/div/*[self::and or self::or]").unwrap().len(), 2);
    assert_eq!(evaluate(&e, e.root(), "count(/div/*) div 2").unwrap(), Value::Number(2.0));
    // Hyphenated names versus subtraction.
    let h = Document::from_str("<r><a-b>5</a-b><a>3</a><b>1</b></r>").unwrap();
    assert_eq!(evaluate(&h, h.root(), "/r/a-b").unwrap().to_number(&h), 5.0);
    assert_eq!(evaluate(&h, h.root(), "/r/a - /r/b").unwrap().to_number(&h), 2.0);
    assert_eq!(evaluate(&h, h.root(), "number(/r/a)-number(/r/b)").unwrap().to_number(&h), 2.0);
    assert_eq!(evaluate(&h, h.root(), "/r/node()").unwrap().as_node_set().unwrap().len(), 3);
    assert_eq!(evaluate(&h, h.root(), "  / r / a  ").unwrap().as_node_set().unwrap().len(), 1);
}

#[test]
fn compiled_expression_is_reusable() {
    let xp = compile("count(//para)").unwrap();
    assert_eq!(xp.source(), "count(//para)");
    let a = Document::from_str("<r><para/></r>").unwrap();
    let b = Document::from_str("<r><para/><para/></r>").unwrap();
    assert_eq!(xp.evaluate(&a, XNode::Node(a.root())).unwrap(), Value::Number(1.0));
    assert_eq!(xp.evaluate(&b, XNode::Node(b.root())).unwrap(), Value::Number(2.0));
}

#[test]
fn value_conversions() {
    let d = doc();
    let v = Value::from(sel(&d, "//n"));
    assert_eq!(v.to_string_value(&d), "1");
    assert!(v.to_boolean());
    assert_eq!(v.to_number(&d), 1.0);
    assert!(!Value::from(Vec::new()).to_boolean());
    assert_eq!(Value::from(true).to_number(&d), 1.0);
    assert_eq!(Value::from("x").as_node_set(), None);
    assert!(Value::from(f64::NAN).to_string_value(&d) == "NaN");
    assert!(!Value::from(f64::NAN).to_boolean());
}
