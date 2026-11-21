//! End-to-end tests over the synthetic fixtures in `tests/fixtures`.

use super::*;
use std::path::PathBuf;

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel)
}

fn book() -> Grammar {
    let g = Grammar::from_xsd(fixture("xsd/book.xsd")).expect("book.xsd loads");
    assert!(
        g.warnings().is_empty(),
        "unexpected warnings: {:?}",
        g.warnings()
    );
    g
}

fn doc(xml: &str) -> Document {
    Document::from_str(xml).expect("well-formed test document")
}

fn messages(g: &Grammar, xml: &str) -> Vec<String> {
    g.validate(&doc(xml))
        .into_iter()
        .map(|e| e.message)
        .collect()
}

/// Finds the first element named `name` (document order).
fn find(d: &Document, name: &str) -> NodeId {
    d.descendants(d.root())
        .find(|&n| d.name(n) == Some(name))
        .unwrap_or_else(|| panic!("no <{name}>"))
}

const VALID_BOOK: &str = r#"<book version="1.0" id="b1" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:noNamespaceSchemaLocation="book.xsd">
  <title>Guide</title>
  <meta><pages>3</pages><author>A. Writer</author><year>2001</year></meta>
  <chapter id="c1">
    <title>Intro <emph role="strong">now</emph></title>
    <para id="p1">See <xref target="c1"/>.</para>
    <list type="bullet"><item><para/></item><item><para>two</para></item></list>
    <warning><para>Hot</para></warning>
    <figure><title><para>Fig</para></title><graphic width="10.5px" scale="2" kind="raster"/></figure>
    <section><para/><section><title>Deep</title><list type="ordered"><item><para/></item><item><para/></item></list></section></section>
    <extension><lax><para>known</para><unknown><anything/></unknown></lax><skip foo="bar"><x y="z"/></skip></extension>
  </chapter>
</book>"#;

#[test]
fn grammar_is_send_and_sync() {
    fn check<T: Send + Sync>() {}
    check::<Grammar>();
}

#[test]
fn valid_document_has_no_errors() {
    let g = book();
    assert_eq!(messages(&g, VALID_BOOK), Vec::<String>::new());
}

#[test]
fn reports_content_model_errors() {
    let g = book();
    let errs = g.validate(&doc(
        r#"<book version="1.0"><chapter><title/><para/></chapter></book>"#,
    ));
    assert_eq!(errs.len(), 1);
    assert_eq!(
        errs[0].message,
        "Element 'chapter' is not allowed here; expected: title"
    );
    assert!(matches!(errs[0].kind, ErrorKind::ElementNotAllowed { .. }));

    let m = messages(
        &g,
        r#"<book version="1.0"><title/><chapter><title/></chapter></book>"#,
    );
    assert_eq!(
        m,
        vec![
            "Element 'chapter' is incomplete; expected one of: para, list, figure, warning, caution, extension, section"
        ]
    );

    let m = messages(
        &g,
        r#"<book version="1.0"><title/><chapter><title/><para/><bogus/></chapter></book>"#,
    );
    assert_eq!(m, vec!["Element 'bogus' is not declared"]);

    // An abstract head is never allowed directly.
    let m = messages(
        &g,
        r#"<book version="1.0"><title/><chapter><title/><admonition><para/></admonition></chapter></book>"#,
    );
    assert_eq!(m.len(), 2, "{m:?}");
    assert!(
        m[0].starts_with("Element 'admonition' is not allowed here"),
        "{m:?}"
    );
    assert!(m[1].starts_with("Element 'chapter' is incomplete"), "{m:?}");
}

#[test]
fn reports_attribute_errors() {
    let g = book();
    let m = messages(
        &g,
        r#"<book version="x" status="wip" color="red"><title/><chapter id="c"><title/>
           <list><item><para/></item><item><para id="c"/></item></list>
           <para><xref target="nowhere"/></para>
           <figure><title><para/></title><graphic width="10 px" scale="0" kind="vector"/></figure>
           </chapter></book>"#,
    );
    let expected = [
        "Value 'x' is not valid for attribute 'version' (must match pattern '\\d+\\.\\d+')",
        "Value 'wip' is not valid for attribute 'status' (allowed: draft, final)",
        "Attribute 'color' is not declared for element 'book'",
        "Required attribute 'type' missing",
        "Duplicate ID 'c'",
        "Value '10 px' is not valid for attribute 'width' (must match pattern '\\d+(\\.\\d+)?(px|pt|mm)')",
        "Value '0' is not valid for attribute 'scale' (must be greater than 0)",
        "Value 'vector' is not valid for attribute 'kind' (must be 'raster')",
        "Attribute 'target' refers to ID 'nowhere', which does not exist",
    ];
    for e in expected {
        assert!(m.iter().any(|x| x == e), "missing {e:?} in {m:#?}");
    }
    assert_eq!(m.len(), expected.len(), "{m:#?}");
}

#[test]
fn reports_text_and_facet_errors() {
    let g = book();
    let m = messages(
        &g,
        r#"<book version="1.0"><title/><meta><author/><pages>0</pages><year>1800</year></meta>
           <chapter><title/>stray text<para/></chapter></book>"#,
    );
    assert!(
        m.contains(&"Content '0' of element 'pages' is not valid (must be at least 1)".to_string()),
        "{m:?}"
    );
    assert!(
        m.contains(
            &"Content '1800' of element 'year' is not valid (must be at least 1900)".to_string()
        ),
        "{m:?}"
    );
    assert!(
        m.contains(&"Text is not allowed in element 'chapter'".to_string()),
        "{m:?}"
    );
    assert_eq!(m.len(), 3, "{m:?}");
}

#[test]
fn reports_wrong_root_and_all_group_errors() {
    let g = book();
    let m = messages(&g, "<chapter><title/><para/></chapter>");
    assert!(m.is_empty(), "any global element may be a root: {m:?}");
    let m = messages(&g, "<nothing/>");
    assert_eq!(m.len(), 1);
    assert!(m[0].starts_with(
        "Element 'nothing' is not allowed as the root element; expected one of: appendix, book"
    ));
    let m = messages(
        &g,
        r#"<book version="1.0"><title/><meta><author/><author/></meta><chapter><title/><para/></chapter></book>"#,
    );
    assert_eq!(
        m,
        vec![
            "Element 'author' is not allowed here; expected one of: year, pages",
            "Element 'meta' is incomplete; expected one of: year, pages",
        ]
    );
}

#[test]
fn context_sensitive_declarations() {
    let g = book();
    let d = doc(VALID_BOOK);
    let book_title = find(&d, "title");
    let t = g.element_decl_for(&d, book_title).unwrap();
    assert_eq!(g.content_type(t), ContentType::TextOnly);
    assert!(!g.decl(t).is_global());

    let figure = find(&d, "figure");
    let fig_title = d.element_children(figure).next().unwrap();
    let ft = g.element_decl_for(&d, fig_title).unwrap();
    assert_eq!(g.content_type(ft), ContentType::ElementOnly);
    assert_ne!(t, ft);

    let chapter_title = d.element_children(find(&d, "chapter")).next().unwrap();
    assert_eq!(
        g.content_type(g.element_decl_for(&d, chapter_title).unwrap()),
        ContentType::Mixed
    );

    let book = g.global_decl("book").unwrap();
    assert_eq!(g.documentation(book), Some("The root of a book."));
    assert_eq!(g.documentation_for_name("para"), Some("A paragraph."));
    let attrs = g.attributes(book);
    let id = attrs.iter().find(|a| a.name == "id").unwrap();
    assert_eq!(id.kind, AttrKind::Id);
    assert_eq!(id.documentation.as_deref(), Some("Unique identifier."));
    let status = attrs.iter().find(|a| a.name == "status").unwrap();
    assert_eq!(status.kind, AttrKind::Enumeration);
    assert_eq!(status.enumeration(), vec!["draft", "final"]);
    assert_eq!(status.default.as_deref(), Some("draft"));
    assert!(attrs.iter().any(|a| a.name == "version" && a.required));

    // The substitution group member inherits the head's type.
    let warning = g.global_decl("warning").unwrap();
    assert_eq!(g.content_type(warning), ContentType::ElementOnly);
    assert_eq!(
        g.content_type(g.global_decl("emph").unwrap()),
        ContentType::TextOnly
    );
    assert_eq!(
        g.content_type(g.global_decl("xref").unwrap()),
        ContentType::Empty
    );

    // Extension appends content and attributes.
    let appendix = g.global_decl("appendix").unwrap();
    assert!(
        g.attributes(appendix)
            .iter()
            .any(|a| a.name == "letter" && a.required)
    );
    assert!(g.attributes(appendix).iter().any(|a| a.name == "id"));
    let m = messages(
        &g,
        r#"<appendix letter="A"><para/><signoff>x</signoff></appendix>"#,
    );
    assert!(m.is_empty(), "{m:?}");
    let m = messages(&g, r#"<appendix letter="AB"><para/></appendix>"#);
    assert_eq!(m.len(), 2, "{m:?}");

    assert!(g.all_element_names().contains(&"graphic".to_string()));
    assert_eq!(
        g.root_candidates().first().map(String::as_str),
        Some("appendix")
    );
    assert!(!g.root_candidates().contains(&"admonition".to_string()));
}

#[test]
fn insertable_and_can_insert() {
    let g = book();
    let d = doc(r#"<book version="1"><title/><chapter><title/><para/></chapter></book>"#);
    let book = d.document_element().unwrap();
    // Between <title/> (child 0) and <chapter> (child 1).
    let ins = g.insertable_at(&d, book, 1);
    let names: Vec<(&str, bool)> = ins
        .iter()
        .map(|i| (i.name.as_str(), i.keeps_valid))
        .collect();
    assert_eq!(names, vec![("chapter", true), ("meta", true)]);
    assert!(g.can_insert(&d, book, 1, "meta"));
    assert!(!g.can_insert(&d, book, 0, "chapter"));
    assert!(g.can_insert(&d, book, 2, "chapter"));
    assert!(!g.can_insert(&d, book, 2, "para"));

    let chapter = find(&d, "chapter");
    let ins = g.insertable_at(&d, chapter, 2);
    let names: Vec<&str> = ins.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "caution",
            "extension",
            "figure",
            "list",
            "para",
            "section",
            "warning"
        ]
    );
    assert!(ins.iter().all(|i| i.keeps_valid));
    // Before the title only a second title is viable, and it breaks validity.
    let ins = g.insertable_at(&d, chapter, 0);
    assert_eq!(
        ins.iter()
            .map(|i| (i.name.as_str(), i.keeps_valid))
            .collect::<Vec<_>>(),
        vec![("title", false)]
    );

    assert!(g.can_contain_text(&d, find(&d, "para")));
    assert!(!g.can_contain_text(&d, chapter));
    // Empty document: root candidates.
    let empty = Document::new();
    assert!(
        g.insertable_at(&empty, empty.root(), 0)
            .iter()
            .any(|i| i.name == "book")
    );
}

#[test]
fn keeps_valid_distinguishes_breaking_insertions() {
    let g = book();
    // list allows at most five items.
    let d = doc(
        r#"<list type="bullet"><item><para/></item><item><para/></item><item><para/></item><item><para/></item><item><para/></item></list>"#,
    );
    let list = d.document_element().unwrap();
    let ins = g.insertable_at(&d, list, 5);
    assert!(ins.is_empty(), "{ins:?} {:?}", g.validate(&d));
    let d = doc(r#"<list type="bullet"><item><para/></item></list>"#);
    let ins = g.insertable_at(&d, d.document_element().unwrap(), 1);
    assert_eq!(ins.len(), 1);
    assert!(
        ins[0].keeps_valid,
        "fixing an incomplete list keeps it valid"
    );

    let d = doc(r#"<book version="1"><title/></book>"#);
    // Before the title only another title is viable, and it breaks validity.
    let ins = g.insertable_at(&d, d.document_element().unwrap(), 0);
    assert_eq!(
        ins.iter()
            .map(|i| (i.name.as_str(), i.keeps_valid))
            .collect::<Vec<_>>(),
        vec![("title", false)]
    );
    let ins = g.insertable_at(&d, d.document_element().unwrap(), 1);
    assert!(ins.iter().all(|i| i.keeps_valid), "{ins:?}");
}

#[test]
fn surround_and_rename() {
    let g = book();
    let d = doc(
        r#"<chapter><title/><para/><para/><list type="bullet"><item><para/></item><item><para/></item></list></chapter>"#,
    );
    let chapter = d.document_element().unwrap();
    // Wrap the two paras in a section or a warning.
    assert!(g.can_surround(&d, chapter, 1, 3, "section"));
    assert!(g.can_surround(&d, chapter, 1, 3, "warning"));
    // A note cannot hold a list, and the title cannot be wrapped.
    assert!(!g.can_surround(&d, chapter, 1, 4, "warning"));
    assert!(g.can_surround(&d, chapter, 1, 4, "section"));
    assert!(!g.can_surround(&d, chapter, 0, 1, "section"));
    assert!(!g.can_surround(&d, chapter, 1, 3, "item"));

    let para = d.element_children(chapter).nth(1).unwrap();
    // Every other block needs content an empty <para/> does not have.
    assert_eq!(g.valid_renames(&d, para), Vec::<String>::new());
    let d = doc(r#"<chapter><title/><warning><para>x</para></warning></chapter>"#);
    let warning = d
        .element_children(d.document_element().unwrap())
        .nth(1)
        .unwrap();
    assert_eq!(g.valid_renames(&d, warning), vec!["caution", "section"]);
}

#[test]
fn skeletons() {
    let g = book();
    let s = g.required_skeleton(g.global_decl("book").unwrap());
    assert_eq!(s.name, "book");
    assert_eq!(s.attrs, vec![("version".to_string(), String::new())]);
    let names: Vec<&str> = s.children.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["title", "chapter"]);
    assert!(s.children[0].wants_text);
    let chapter = &s.children[1];
    assert_eq!(
        chapter
            .children
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        vec!["title", "para"]
    );

    let list = g.required_skeleton(g.global_decl("list").unwrap());
    assert_eq!(list.attrs, vec![("type".to_string(), "bullet".to_string())]);
    assert_eq!(list.children.len(), 2);
    assert_eq!(list.children[0].children[0].name, "para");

    // Recursive types terminate.
    let sec = g.required_skeleton(g.global_decl("section").unwrap());
    assert_eq!(sec.children.len(), 1);
    let meta = g.required_skeleton(g.global_decl("meta").unwrap());
    assert_eq!(
        meta.children
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        vec!["author", "pages"]
    );
}

#[test]
fn namespaces_imports_and_catalog() {
    let cat = Catalog::load(fixture("ns/catalog.toml")).unwrap();
    let g = Grammar::from_xsd_with_catalog(fixture("ns/dmodule.xsd"), &cat).unwrap();
    assert!(g.warnings().is_empty(), "{:?}", g.warnings());
    let xml = r#"<dmodule xmlns:xlink="http://www.w3.org/1999/xlink" xmlns:d="http://www.purl.org/dc/elements/1.1/" xml:lang="en">
        <d:title>T</d:title>
        <content><refs><dmRef xlink:href="DMC-1" xlink:type="simple"/><dmRef xlink:title="x"/></refs><para>p</para></content>
      </dmodule>"#;
    let m = messages(&g, xml);
    assert_eq!(m, vec!["Required attribute 'xlink:href' missing"]);
    let m = messages(
        &g,
        r#"<dmodule><title/><content><para/></content></dmodule>"#,
    );
    assert_eq!(m, vec!["Element 'title' is not declared"]);
    let m = messages(
        &g,
        r#"<dmodule xmlns:l="http://www.w3.org/1999/xlink"><content><refs><dmRef l:href="x" l:type="extended"/></refs><para/></content></dmodule>"#,
    );
    assert_eq!(
        m,
        vec!["Value 'extended' is not valid for attribute 'l:type' (must be 'simple')"]
    );

    // Insertion names use the prefix bound in the document.
    let d = doc(xml);
    let root = d.document_element().unwrap();
    let ins = g.insertable_at(&d, root, 0);
    assert_eq!(
        ins.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(),
        vec!["content", "d:title"]
    );

    // Without the catalog the remote location falls back to a same-named
    // local file; a location that cannot be found at all is only a warning.
    let g = Grammar::from_xsd(fixture("ns/dmodule.xsd")).unwrap();
    assert!(g.warnings().is_empty(), "{:?}", g.warnings());
    let text = std::fs::read_to_string(fixture("ns/dmodule.xsd"))
        .unwrap()
        .replace("xlink.xsd", "missing.xsd");
    let tmp = std::env::temp_dir().join(format!("xylem-schema-ns-{}.xsd", std::process::id()));
    std::fs::write(&tmp, text).unwrap();
    let g = Grammar::from_xsd(&tmp).unwrap();
    std::fs::remove_file(&tmp).ok();
    assert!(
        g.warnings().iter().any(|w| w.contains("missing.xsd")),
        "{:?}",
        g.warnings()
    );
}

#[test]
fn redefine() {
    let g = Grammar::from_xsd(fixture("redefine/main.xsd")).unwrap();
    assert!(g.warnings().is_empty(), "{:?}", g.warnings());
    assert!(messages(&g, "<notes><note><text/><author/></note><a/><b/></notes>").is_empty());
    assert_eq!(
        messages(&g, "<notes><note><text/></note><a/><b/></notes>").len(),
        1
    );
}

#[test]
fn for_document_resolves_schema_location() {
    let xml = VALID_BOOK;
    let d = doc(xml);
    let path = fixture("xsd/instance.xml");
    let g = Grammar::for_document(&d, Some(&path), &Catalog::default())
        .unwrap()
        .unwrap();
    assert!(g.validate(&d).is_empty(), "{:?}", g.validate(&d));
    // A stale path falls back to the same file name next to the document.
    let d = doc(
        r#"<book xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:noNamespaceSchemaLocation="../../old%20place/book.xsd" version="1.0"><title/><chapter><title/><para/></chapter></book>"#,
    );
    let g = Grammar::for_document(&d, Some(&path), &Catalog::default())
        .unwrap()
        .unwrap();
    assert!(g.validate(&d).is_empty(), "{:?}", g.validate(&d));
    assert!(Grammar::for_document(&doc("<x/>"), Some(&path), &Catalog::default()).is_none());
}

// ------------------------------------------------------------------ DTD

fn descript() -> Grammar {
    Grammar::from_dtd(fixture("dtd/descript.dtd")).expect("DTD loads")
}

#[test]
fn dtd_parameter_entities_and_conditional_sections() {
    let g = descript();
    assert!(g.warnings().is_empty(), "{:?}", g.warnings());
    assert_eq!(g.kind(), GrammarKind::Dtd);
    assert!(g.global_decl("legacy").is_some());
    assert!(g.global_decl("remarks").is_none(), "IGNOREd via %draft;");
    assert!(g.global_decl("ignored").is_none());
    let dm = g.global_decl("dmodule").unwrap();
    let names: Vec<&str> = g.attributes(dm).iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, vec!["id", "status", "lang", "xmlns:xsi", "version"]);
    let version = &g.attributes(dm)[4];
    assert_eq!(version.fixed.as_deref(), Some("2.3"));
    let status = &g.attributes(dm)[1];
    assert_eq!(status.kind, AttrKind::Enumeration);
    assert_eq!(status.default.as_deref(), Some("draft"));
    assert_eq!(g.documentation(dm), Some("The data module root."));
    assert_eq!(g.root_candidates(), vec!["dmodule", "legacy"]);

    let graphic = g.global_decl("graphic").unwrap();
    let fmt = g
        .attributes(graphic)
        .iter()
        .find(|a| a.name == "fmt")
        .unwrap();
    assert_eq!(fmt.kind, AttrKind::Notation);
    assert_eq!(fmt.enumeration(), vec!["cgm", "tiff"]);
    assert_eq!(g.attributes(graphic)[0].kind, AttrKind::Entity);

    assert_eq!(
        g.entity("eacute").map(|e| &e.kind),
        Some(&EntityKind::Internal("é".into()))
    );
    assert_eq!(
        g.entity("company").map(|e| &e.kind),
        Some(&EntityKind::Internal("Acme &amp; Co".into()))
    );
    assert!(matches!(
        g.entity("chapter1").map(|e| &e.kind),
        Some(EntityKind::External { .. })
    ));
    assert_eq!(g.notations().len(), 2);
    assert_eq!(
        g.content_type(g.global_decl("para").unwrap()),
        ContentType::Mixed
    );
    assert_eq!(
        g.content_type(g.global_decl("title").unwrap()),
        ContentType::TextOnly
    );
    assert_eq!(
        g.content_type(g.global_decl("xref").unwrap()),
        ContentType::Empty
    );
}

#[test]
fn dtd_validation_with_internal_subset() {
    let xml = r#"<?xml version="1.0"?>
<!DOCTYPE dmodule SYSTEM "descript.dtd" [
  <!ENTITY % draft "INCLUDE">
  <!ENTITY fig1 SYSTEM "fig1.cgm" NDATA cgm>
  <!ATTLIST para extra CDATA #IMPLIED>
]>
<dmodule id="d1" version="2.3">
  <idstatus>x &eacute; &company;</idstatus>
  <content>
    <para extra="1">Text <xref xrefid="d1"/> &undefined;</para>
    <randlist><item><para/></item><item><para/></item></randlist>
    <note><para/></note>
    <figure><graphic boardno="fig1" fmt="cgm"/></figure>
  </content>
</dmodule>"#;
    let d = doc(xml);
    let path = fixture("dtd/instance.xml");
    let g = Grammar::for_document(&d, Some(&path), &Catalog::new())
        .unwrap()
        .unwrap();
    assert!(
        g.global_decl("remarks").is_some(),
        "internal subset overrides %draft;"
    );
    let m: Vec<String> = g.validate(&d).into_iter().map(|e| e.message).collect();
    assert_eq!(m, vec!["Entity '&undefined;' is not declared"]);

    let bad = r#"<!DOCTYPE dmodule SYSTEM "descript.dtd">
<dmodule version="9" status="other">
  <idstatus><para/></idstatus>
  <content><randlist><item><para/></item></randlist><figure><graphic boardno="nope" fmt="png"/></figure>
  <para><xref xidtype="table"/></para></content>
</dmodule>"#;
    let d = doc(bad);
    let g = Grammar::for_document(&d, Some(&path), &Catalog::new())
        .unwrap()
        .unwrap();
    let m: Vec<String> = g.validate(&d).into_iter().map(|e| e.message).collect();
    let expected = [
        "Value 'other' is not valid for attribute 'status' (allowed: draft, final)",
        "Value '9' is not valid for attribute 'version' (must be '2.3')",
        "Element 'para' is not allowed here; 'idstatus' may contain only text",
        "Element 'randlist' is incomplete; expected one of: item",
        "Value 'nope' is not valid for attribute 'boardno' ('nope' is not a declared unparsed entity)",
        "Value 'png' is not valid for attribute 'fmt' (allowed: cgm, tiff)",
        "Element 'para' is not allowed here; expected: figure",
        "Required attribute 'xrefid' missing",
    ];
    for e in expected {
        assert!(m.iter().any(|x| x == e), "missing {e:?} in {m:#?}");
    }
    assert_eq!(m.len(), expected.len(), "{m:#?}");

    let wrong_root = doc(r#"<!DOCTYPE dmodule SYSTEM "descript.dtd"><para/>"#);
    let g = Grammar::for_document(&wrong_root, Some(&path), &Catalog::new())
        .unwrap()
        .unwrap();
    assert!(matches!(
        g.validate(&wrong_root)[0].kind,
        ErrorKind::WrongRoot { .. }
    ));
}

#[test]
fn dtd_editing_queries() {
    let g = descript();
    let d = doc("<content><para/><figure><graphic boardno='x'/></figure></content>");
    let content = d.document_element().unwrap();
    let ins = g.insertable_at(&d, content, 1);
    let names: Vec<(&str, bool)> = ins
        .iter()
        .map(|i| (i.name.as_str(), i.keeps_valid))
        .collect();
    assert_eq!(
        names,
        vec![
            ("figure", true),
            ("note", true),
            ("para", true),
            ("randlist", true)
        ]
    );
    let ins = g.insertable_at(&d, content, 2);
    assert_eq!(
        ins.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(),
        vec!["figure"]
    );
    let s = g.required_skeleton(g.global_decl("dmodule").unwrap());
    let flat: Vec<&str> = s.children.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(flat, vec!["idstatus", "content"]);
    assert_eq!(s.attrs, vec![]);
    let r = g.required_skeleton(g.global_decl("randlist").unwrap());
    assert_eq!(r.children.len(), 2);
    let gr = g.required_skeleton(g.global_decl("graphic").unwrap());
    assert_eq!(gr.attrs, vec![("boardno".to_string(), String::new())]);
    assert!(g.can_surround(&d, content, 0, 1, "note"));
    assert!(!g.can_surround(&d, content, 1, 2, "note"));
    let d = doc("<content><note><para/></note></content>");
    let note = d
        .element_children(d.document_element().unwrap())
        .next()
        .unwrap();
    assert_eq!(g.valid_renames(&d, note), Vec::<String>::new());
}

#[test]
fn dtd_text_with_inline_parameter_entities() {
    let text = r#"
        <!ENTITY % inline "b | i">
        <!ENTITY % yes "INCLUDE">
        <![%yes;[ <!ELEMENT doc (p+)> ]]>
        <!ELEMENT p (#PCDATA | %inline;)*>
        <!ELEMENT b (#PCDATA)> <!ELEMENT i (#PCDATA)>
        <!ATTLIST p n NMTOKEN #REQUIRED  ref IDREFS #IMPLIED  id ID #IMPLIED>
        <!ATTLIST p n CDATA #IMPLIED>
    "#;
    let g = Grammar::from_dtd_text(text, ".").unwrap();
    assert!(g.warnings().is_empty(), "{:?}", g.warnings());
    let m = messages(
        &g,
        r#"<doc><p n="a" id="x">t<b/>u<i>v</i></p><p n="b c" ref="x y"/></doc>"#,
    );
    assert_eq!(
        m,
        vec![
            "Value 'b c' is not valid for attribute 'n' (must be a valid NMTOKEN)",
            "Attribute 'ref' refers to ID 'y', which does not exist",
        ]
    );
    assert_eq!(g.root_candidates(), vec!["doc"]);
}

#[test]
fn oasis_catalog_resolves_doctype_public_id() {
    let dir = std::env::temp_dir().join(format!("xylem-schema-cat-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cat_path = dir.join("catalog.xml");
    let dtd = fixture("dtd/descript.dtd");
    std::fs::write(
        &cat_path,
        format!(
            r#"<catalog xmlns="urn:oasis:names:tc:entity:xmlns:xml:catalog">
                 <public publicId="-//S1000D//DTD Descriptive Data Module 2.3//EN" uri="file:///{}"/>
               </catalog>"#,
            dtd.display().to_string().replace('\\', "/").trim_start_matches('/')
        ),
    )
    .unwrap();
    let cat = Catalog::load(&cat_path).unwrap();
    let d = doc(
        r#"<!DOCTYPE dmodule PUBLIC "-//S1000D//DTD Descriptive Data Module 2.3//EN" "http://example.com/descript.dtd"><dmodule version="2.3"><idstatus/><content><para/></content></dmodule>"#,
    );
    let g = Grammar::for_document(&d, None, &cat).unwrap().unwrap();
    assert!(g.validate(&d).is_empty(), "{:?}", g.validate(&d));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn complex_and_simple_content_restriction() {
    let g = Grammar::from_xsd(fixture("xsd/restrict.xsd")).unwrap();
    assert!(g.warnings().is_empty(), "{:?}", g.warnings());
    let ok = r#"<root><r x="1"><a/></r><m unit="mm">10</m><graphic src="g"/></root>"#;
    let m = messages(&g, ok);
    // No DTD entities are known here, so the ENTITY value cannot resolve.
    assert_eq!(
        m,
        vec!["Value 'g' is not valid for attribute 'src' ('g' is not a declared unparsed entity)"]
    );
    let m = messages(
        &g,
        r#"<root><r y="1"><a/><b/></r><m unit="mm">11</m><graphic src="g"/></root>"#,
    );
    assert!(
        m.contains(&"Attribute 'y' is not declared for element 'r'".to_string()),
        "{m:?}"
    );
    assert!(
        m.contains(
            &"Element 'b' is not allowed here; no more elements are allowed in 'r'".to_string()
        ),
        "{m:?}"
    );
    assert!(
        m.contains(&"Content '11' of element 'm' is not valid (must be at most 10)".to_string()),
        "{m:?}"
    );
    let mdecl = g.global_decl("m").unwrap();
    assert_eq!(g.content_type(mdecl), ContentType::TextOnly);
    assert!(
        g.attributes(mdecl).iter().any(|a| a.name == "unit"),
        "simpleContent restriction keeps base attributes"
    );
}

#[test]
fn xsd_document_with_internal_subset() {
    let xml = r#"<!DOCTYPE root [
  <!NOTATION cgm SYSTEM "cgm">
  <!ENTITY fig1 SYSTEM "fig1.cgm" NDATA cgm>
  <!ENTITY product "Widget">
  <!ATTLIST label extra CDATA #IMPLIED>
]>
<root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:noNamespaceSchemaLocation="restrict.xsd">
  <r><a/></r><m>1</m><graphic src="fig1"/><label extra="yes">&product;</label><note extra="no"/>
</root>"#;
    let d = doc(xml);
    let g = Grammar::for_document(&d, Some(&fixture("xsd/instance.xml")), &Catalog::new())
        .unwrap()
        .unwrap();
    assert!(
        matches!(g.entity("fig1").map(|e| &e.kind), Some(EntityKind::Unparsed { notation, .. }) if notation == "cgm")
    );
    assert_eq!(g.notations().len(), 1);
    // The extra attribute lands on <label> only, not on other xs:string elements.
    let m = messages(&g, xml);
    assert_eq!(
        m,
        vec!["Attribute 'extra' is not declared for element 'note'"]
    );
    let bad = xml.replace("src=\"fig1\"", "src=\"fig2\"");
    let m = messages(&g, &bad);
    assert!(
        m.iter()
            .any(|x| x.contains("'fig2' is not a declared unparsed entity")),
        "{m:?}"
    );
}

#[test]
fn xsi_schema_location_pairs() {
    let xml = r#"<n:notes xmlns:n="urn:example:notes" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
        xsi:schemaLocation="urn:example:notes notes-ns.xsd" count="2"><n:note/><n:note/></n:notes>"#;
    let d = doc(xml);
    let g = Grammar::for_document(&d, Some(&fixture("xsd/instance.xml")), &Catalog::new())
        .unwrap()
        .unwrap();
    assert!(g.validate(&d).is_empty(), "{:?}", g.validate(&d));
    // Default-namespace form of the same document.
    let m = messages(
        &g,
        r#"<notes xmlns="urn:example:notes"><note/><other/></notes>"#,
    );
    assert_eq!(m, vec!["Element 'other' is not declared"]);
    // Unqualified names are in no namespace and do not match.
    let m = messages(&g, "<notes><note/></notes>");
    assert!(
        matches!(
            g.validate(&doc("<notes/>"))[0].kind,
            ErrorKind::WrongRoot { .. }
        ),
        "{m:?}"
    );
    let ins = g.insertable_at(&d, d.document_element().unwrap(), 0);
    assert_eq!(
        ins.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(),
        vec!["n:note"]
    );
}

#[test]
fn non_ascii_values_never_panic() {
    let t = datatype::TypeTable::new();
    let samples = [
        "日日x",
        "2024-01-01日日日",
        "é+01:00",
        "--日-01",
        "%e日",
        "ab%",
        "\u{FEFF}",
        "",
    ];
    for id in 0..t.types.len() as u32 {
        for s in samples {
            let _ = t.validate(TypeId(id), s);
        }
    }
    for s in samples {
        let _ = resolve::percent_decode(s);
        let _ = Grammar::from_dtd_text(&format!("<!-- {s}"), ".");
        let _ = Grammar::from_dtd_text(&format!("<!ELEMENT {s} (#PCDATA)> %{s}; <![{s}["), ".");
    }
    assert_eq!(resolve::percent_decode("%e日"), "%e日");
}
