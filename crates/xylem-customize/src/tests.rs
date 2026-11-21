//! Cross-module tests using synthetic fixtures.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use xylem_dom::Document;

use crate::*;

/// A uniquely named scratch directory removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("xylem-customize-test-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn write(&self, rel: &str, content: impl AsRef<[u8]>) -> PathBuf {
        let path = self.0.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn manifest(name: &str, matching: &str) -> String {
    format!("[doctype]\nname = \"{name}\"\n\n[match]\n{matching}\n")
}

// ------------------------------------------------------------------ package

fn full_package() -> DoctypePackage {
    let mut pkg = DoctypePackage::new(DoctypeInfo {
        name: "full".into(),
        description: "Everything".into(),
        version: "2.1".into(),
    });
    pkg.matching = MatchRules {
        roots: vec!["book".into()],
        schema_locations: vec!["full*.xsd".into()],
        public_ids: vec!["-//ACME//".into()],
        file_names: vec!["*.book.xml".into()],
        priority: 5,
    };
    pkg.files = Files {
        schema: Some("full.xsd".into()),
        style: Some("full.css".into()),
        structure_style: Some("sub/full_structure.css".into()),
        entities: Some("entities.ent".into()),
    };
    pkg.templates.insert(
        "para".into(),
        Template::new("<para><?xm-replace_text Type new para here ?></para>"),
    );
    let b = &mut pkg.behaviour;
    b.followed_by.insert("title".into(), "para".into());
    b.paragraphs.push("para".into());
    b.bulleted_lists.push(ListDef {
        name: "ul".into(),
        header: Some("title".into()),
        item: "li".into(),
    });
    b.numbered_lists.push(ListDef {
        name: "ol".into(),
        header: None,
        item: "li".into(),
    });
    b.definition_lists.push(DefinitionListDef {
        name: "dl".into(),
        header: None,
        term: "dt".into(),
        definition: "dd".into(),
    });
    b.toggles.insert("bold".into(), "strong".into());
    b.images.push(ImageDef {
        element: "graphic".into(),
        source_attr: "src".into(),
        width_attr: Some("w".into()),
        height_attr: None,
    });
    b.read_only_elements.push("meta".into());
    b.non_removable_elements.push("title".into());
    b.id_generation = Some(IdGeneration {
        elements: vec!["para".into()],
        attr: "id".into(),
        pattern: "{name}-{seq:4}".into(),
    });
    b.auto_ids = true;
    pkg.pretty_print.indent_amount = Some(2);
    pkg.pretty_print.elements.insert(
        "para".into(),
        ElementPrettyPrint {
            indent_content: true,
            newline_before_start_tag: false,
            newline_before_end_tag: true,
            tag_color: Some("#fff".into()),
        },
    );
    pkg.descriptions.insert("para".into(), "Paragraph".into());
    pkg.toolbar = default_toolbar(&pkg.behaviour);
    pkg.toolbar.push(ToolbarEntry {
        id: "wrap".into(),
        label: "Wrap".into(),
        action: Action::Surround("note".into()),
        element: Some("note".into()),
        icon: None,
        shortcut: Some("ctrl+k".into()),
    });
    pkg
}

#[test]
fn full_package_round_trips_through_toml() {
    let dir = TempDir::new();
    let pkg = full_package();
    pkg.save(dir.path()).unwrap();
    let text = fs::read_to_string(dir.path().join(MANIFEST_NAME)).unwrap();
    assert!(text.contains("[templates.para]"), "{text}");
    assert!(text.contains("action = \"surround:note\""), "{text}");

    let loaded = DoctypePackage::load(dir.path()).unwrap();
    assert!(loaded.base_dir().unwrap().is_absolute());
    assert_eq!(loaded.doctype, pkg.doctype);
    assert_eq!(loaded.matching, pkg.matching);
    assert_eq!(loaded.files, pkg.files);
    assert_eq!(loaded.templates, pkg.templates);
    assert_eq!(loaded.behaviour, pkg.behaviour);
    assert_eq!(loaded.pretty_print, pkg.pretty_print);
    assert_eq!(loaded.descriptions, pkg.descriptions);
    assert_eq!(loaded.toolbar, pkg.toolbar);
}

#[test]
fn generic_package_round_trips_and_has_default_toolbar() {
    let dir = TempDir::new();
    let generic = DoctypePackage::generic();
    assert!(generic.is_virtual());
    assert!(generic.templates.is_empty());
    assert!(generic.toolbar.iter().any(|e| e.action == Action::Save));
    generic.save(dir.path()).unwrap();
    let loaded = DoctypePackage::load(dir.path()).unwrap();
    assert_eq!(loaded.toolbar, generic.toolbar);
    assert_eq!(loaded.matching, generic.matching);
    assert!(!loaded.is_virtual());
}

#[test]
fn minimal_manifest_gets_defaults() {
    let dir = TempDir::new();
    dir.write(MANIFEST_NAME, "[doctype]\nname = \"min\"\n");
    let pkg = DoctypePackage::load(dir.path()).unwrap();
    assert_eq!(pkg.doctype.version, "1.0");
    assert!(pkg.toolbar.is_empty());
    assert!(!pkg.effective_toolbar().is_empty());
    assert_eq!(pkg.resolved_files(), ResolvedFiles::default());
}

#[test]
fn load_reports_bad_manifests() {
    let dir = TempDir::new();
    assert!(matches!(
        DoctypePackage::load(dir.path()),
        Err(Error::Io { .. })
    ));
    dir.write(MANIFEST_NAME, "[doctype\nname=");
    assert!(matches!(
        DoctypePackage::load(dir.path()),
        Err(Error::TomlParse { .. })
    ));
    dir.write(
        MANIFEST_NAME,
        "[doctype]\nname = \"x\"\n[files]\nstyle = \"../evil.css\"\n",
    );
    assert!(matches!(
        DoctypePackage::load(dir.path()),
        Err(Error::Invalid(_))
    ));
    dir.write(
        MANIFEST_NAME,
        "[doctype]\nname = \"x\"\n[[toolbar]]\nid = \"a\"\nlabel = \"A\"\naction = \"explode\"\n",
    );
    assert!(matches!(
        DoctypePackage::load(dir.path()),
        Err(Error::TomlParse { .. })
    ));
}

#[test]
fn resolves_paths_and_copies_files_on_save() {
    let src = TempDir::new();
    let dst = TempDir::new();
    src.write(
        MANIFEST_NAME,
        "[doctype]\nname = \"p\"\n[files]\nstyle = \"a.css\"\nschema = \"gone.xsd\"\n",
    );
    src.write("a.css", "x{}");
    let pkg = DoctypePackage::load(src.path()).unwrap();
    let resolved = pkg.resolved_files();
    assert!(resolved.style.as_ref().unwrap().is_absolute());
    assert!(resolved.style.unwrap().ends_with("a.css"));
    assert_eq!(pkg.missing_files(), ["gone.xsd"]);

    pkg.save(dst.path()).unwrap();
    assert_eq!(fs::read_to_string(dst.path().join("a.css")).unwrap(), "x{}");
    assert!(!dst.path().join("gone.xsd").exists());
    // Saving over itself leaves the files alone.
    pkg.save(src.path()).unwrap();
    assert_eq!(fs::read_to_string(src.path().join("a.css")).unwrap(), "x{}");
}

#[test]
fn template_instantiates_into_a_document() {
    let pkg = full_package();
    let mut doc = Document::from_str("<book/>").unwrap();
    let nodes = pkg
        .template("para")
        .unwrap()
        .instantiate_into(&mut doc)
        .unwrap();
    let book = doc.document_element().unwrap();
    for n in &nodes {
        doc.append_child(book, *n);
    }
    assert_eq!(
        doc.to_xml(),
        "<book><para><?xm-replace_text Type new para here ?></para></book>"
    );
}

// ----------------------------------------------------------------- registry

fn doc(xml: &str) -> Document {
    Document::from_str(xml).unwrap()
}

#[test]
fn registry_scans_nested_folders_and_records_failures() {
    let root = TempDir::new();
    root.write("a/doctype.toml", manifest("a", "roots = [\"alpha\"]"));
    root.write("group/b/doctype.toml", manifest("b", "roots = [\"beta\"]"));
    root.write("group/broken/doctype.toml", "not toml [");
    root.write(
        "group/b/inside/doctype.toml",
        manifest("hidden", "roots = [\"h\"]"),
    );
    let reg = DoctypeRegistry::scan(&[root.path().to_path_buf(), root.path().join("missing")]);
    let names: Vec<&str> = reg
        .packages()
        .iter()
        .map(|p| p.doctype.name.as_str())
        .collect();
    assert_eq!(names, ["a", "b"], "packages do not nest");
    assert_eq!(reg.errors().len(), 1);
    assert!(reg.get("b").is_some() && reg.get("hidden").is_none());
}

#[test]
fn match_prefers_schema_location_then_priority_then_root() {
    let root = TempDir::new();
    root.write("pub/doctype.toml", manifest("pub", "roots = [\"pub\"]"));
    root.write(
        "pub-cover/doctype.toml",
        manifest(
            "pub-cover",
            "roots = [\"pub\"]\nschema_locations = [\"pub-cover.xsd\"]",
        ),
    );
    root.write(
        "pub-front/doctype.toml",
        manifest(
            "pub-front",
            "roots = [\"pub\"]\nschema_locations = [\"pub-front.xsd\"]",
        ),
    );
    root.write(
        "pushy/doctype.toml",
        manifest("pushy", "roots = [\"pub\"]\npriority = 9"),
    );
    let reg = DoctypeRegistry::scan(&[root.path().to_path_buf()]);
    let pick = |xml: &str| {
        reg.match_document(&doc(xml), None)
            .map(|p| p.doctype.name.clone())
    };

    // Priority wins over the plain root match...
    assert_eq!(pick("<pub/>").as_deref(), Some("pushy"));
    // ...but not over nothing: a schema hit at equal priority needs equal priority.
    let low = TempDir::new();
    low.write("pub/doctype.toml", manifest("pub", "roots = [\"pub\"]"));
    low.write(
        "pub-cover/doctype.toml",
        manifest(
            "pub-cover",
            "roots = [\"pub\"]\nschema_locations = [\"pub-cover.xsd\"]",
        ),
    );
    low.write(
        "pub-front/doctype.toml",
        manifest(
            "pub-front",
            "roots = [\"pub\"]\nschema_locations = [\"pub-front.xsd\"]",
        ),
    );
    let reg = DoctypeRegistry::scan(&[low.path().to_path_buf()]);
    let pick = |xml: &str| {
        reg.match_document(&doc(xml), None)
            .map(|p| p.doctype.name.clone())
    };
    assert_eq!(pick("<pub/>").as_deref(), Some("pub"));
    assert_eq!(
        pick(r#"<pub xmlns:xsi="x" xsi:noNamespaceSchemaLocation="../schemas/PUB-Cover.xsd"/>"#)
            .as_deref(),
        Some("pub-cover")
    );
    assert_eq!(
        pick(r#"<pub xmlns:xsi="x" xsi:schemaLocation="urn:n C:\s\pub-front.xsd"/>"#).as_deref(),
        Some("pub-front")
    );
    assert_eq!(pick("<other/>"), None);
}

#[test]
fn match_by_public_id_system_id_file_name_and_prefixed_root() {
    let root = TempDir::new();
    root.write(
        "dtd/doctype.toml",
        manifest("dtd", "public_ids = [\"-//ACME//DTD Memo\"]"),
    );
    root.write(
        "sys/doctype.toml",
        manifest("sys", "schema_locations = [\"memo.dtd\"]"),
    );
    root.write(
        "file/doctype.toml",
        manifest("file", "file_names = [\"*.memo.xml\"]"),
    );
    root.write("ns/doctype.toml", manifest("ns", "roots = [\"doc\"]"));
    let reg = DoctypeRegistry::scan(&[root.path().to_path_buf()]);
    let pick = |xml: &str, path: Option<&Path>| {
        reg.match_document(&doc(xml), path)
            .map(|p| p.doctype.name.clone())
    };

    assert_eq!(
        pick(
            "<!DOCTYPE m PUBLIC \"-//ACME//DTD Memo 1.0//EN\" \"x.dtd\"><m/>",
            None
        )
        .as_deref(),
        Some("dtd")
    );
    assert_eq!(
        pick("<!DOCTYPE m SYSTEM \"dtds/Memo.dtd\"><m/>", None).as_deref(),
        Some("sys")
    );
    assert_eq!(
        pick("<m/>", Some(Path::new("/x/a.memo.xml"))).as_deref(),
        Some("file")
    );
    assert_eq!(pick("<m/>", Some(Path::new("/x/a.xml"))), None);
    assert_eq!(pick("<p:doc xmlns:p=\"u\"/>", None).as_deref(), Some("ns"));
}

// ----------------------------------------------------------------- importer

const CTM: &str = "\u{feff}<?xml version=\"1.0\"?>
<!DOCTYPE DTDExtensions SYSTEM \"ctm.dtd\">
<DTDExtensions Version=\"2.0\">
  <ElementPropertiesList>
    <GlobalPrettyPrintOptions><IndentAmount>4</IndentAmount></GlobalPrettyPrintOptions>
    <ElementProperties><Name ns=\"\">para</Name><LongDescription>Para</LongDescription><PrettyPrintOptions><IndentContent/></PrettyPrintOptions></ElementProperties>
    <GlobalPrettyPrintOptions></GlobalPrettyPrintOptions>
  </ElementPropertiesList>
  <Templates>
    <Template><Name ns=\"\">para</Name><MiniTemplate><![CDATA[<para><?xm-replace_text Type new para here ?></para>]]></MiniTemplate></Template>
    <Template><Name ns=\"\">cover</Name><MiniTemplate><![CDATA[<cover><title1>x</title></cover>]]></MiniTemplate></Template>
  </Templates>
  <FollowedByList><FollowedByListItem><Name ns=\"\">title</Name><FollowedBy ns=\"\">para</FollowedBy></FollowedByListItem></FollowedByList>
  <Paragraphs><Paragraph><Name ns=\"\">para</Name></Paragraph></Paragraphs>
  <TogglingInlines><Italic><Name ns=\"\">emphasis</Name></Italic></TogglingInlines>
</DTDExtensions>";

const XSD: &str = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="pub"><xs:complexType><xs:sequence><xs:element ref="para"/></xs:sequence></xs:complexType></xs:element>
  <xs:element name="para" type="xs:string"/>
</xs:schema>"#;

/// Writes an XMAX customization folder `<root>/Cust/Set` with a shared
/// `common.css` one level up, as found in the real corpus.
fn make_folder_set(root: &TempDir, set: &str) -> PathBuf {
    root.write(
        "Cust/common.css",
        "p { background: url(images/dot.png); } /* url(ignored.png) */",
    );
    root.write("Cust/images/dot.png", [1u8, 2, 3]);
    root.write(&format!("Cust/{set}/{set}.ctm"), CTM);
    root.write(
        &format!("Cust/{set}/{set}.css"),
        "@import url(\"common.css\");\nb { color: red }",
    );
    root.write(
        &format!("Cust/{set}/{set}_structure.css"),
        "@import url(common.css);\n$DOCUMENT { x: y }",
    );
    root.write(&format!("Cust/{set}/{set}.xsd"), XSD);
    root.write(
        &format!("Cust/{set}/{set}.xdp"),
        format!("\"General\"\n{{\n\"RulesFile\" = \"{set}.xsd\"\n}}\n"),
    );
    root.write(&format!("Cust/{set}/{set}.tbr"), "binary");
    root.write(&format!("Cust/{set}/On_Drop.js"), "");
    root.write(&format!("Cust/{set}/entities.ent"), "<!ENTITY a \"b\">");
    root.path().join("Cust").join(set)
}

#[test]
fn imports_a_folder_customization() {
    let root = TempDir::new();
    let out = TempDir::new();
    let src = make_folder_set(&root, "Set");
    let dir = out.path().join("pkg");
    let report = import_xmax(&src, &dir).unwrap();

    for f in [
        "doctype.toml",
        "Set.css",
        "Set_structure.css",
        "Set.xsd",
        "common.css",
        "dot.png",
        "entities.ent",
    ] {
        assert!(dir.join(f).is_file(), "{f} missing; report:\n{report}");
    }
    // The nested reference was flattened so the copy still resolves.
    let common = fs::read_to_string(dir.join("common.css")).unwrap();
    assert!(
        common.contains("url(dot.png)") && common.contains("url(ignored.png)"),
        "{common}"
    );

    let pkg = DoctypePackage::load(&dir).unwrap();
    assert!(pkg.missing_files().is_empty());
    assert_eq!(pkg.doctype.name, "Set");
    assert_eq!(pkg.matching.roots, ["pub"]);
    assert_eq!(pkg.matching.schema_locations, ["Set.xsd"]);
    assert_eq!(pkg.files.entities.as_deref(), Some("entities.ent"));
    assert_eq!(pkg.behaviour.toggles["italic"], "emphasis");
    assert_eq!(pkg.behaviour.followed_by["title"], "para");
    assert_eq!(pkg.pretty_print.indent_amount, Some(4));
    assert!(pkg.pretty_print.elements["para"].indent_content);
    assert_eq!(pkg.descriptions["para"], "Para");
    assert!(pkg.templates.contains_key("para") && !pkg.templates.contains_key("cover"));
    assert!(
        pkg.toolbar
            .iter()
            .any(|e| e.action == Action::Toggle("italic".into()))
    );

    assert!(
        report.warnings.iter().any(|w| w.contains("`cover`")),
        "{report}"
    );
    for ignored in ["Set.tbr", "On_Drop.js", "Set.xdp"] {
        assert!(
            report.ignored.iter().any(|i| i.starts_with(ignored)),
            "{ignored} not reported:\n{report}"
        );
    }
    assert!(report.to_string().contains("converted"));
}

#[test]
fn imported_package_matches_documents() {
    let root = TempDir::new();
    let out = TempDir::new();
    let src = make_folder_set(&root, "Set");
    import_xmax(&src, &out.path().join("Set")).unwrap();
    let reg = DoctypeRegistry::scan(&[out.path().to_path_buf()]);
    let hit = reg.match_document(&doc("<pub/>"), None).unwrap();
    assert_eq!(hit.doctype.name, "Set");
}

#[test]
fn unresolved_references_warn_without_failing() {
    let root = TempDir::new();
    let out = TempDir::new();
    root.write("S/S.ctm", CTM);
    root.write("S/S.css", "a { content: url(missing.gif) }");
    let report = import_xmax(&root.path().join("S"), out.path()).unwrap();
    assert!(report.warnings.iter().any(|w| w.contains("missing.gif")));
    assert!(report.warnings.iter().any(|w| w.contains("no schema")));
    let pkg = DoctypePackage::load(out.path()).unwrap();
    assert!(pkg.files.schema.is_none() && pkg.matching.roots.is_empty());
}

#[test]
fn rejects_sources_without_ctm_or_with_bad_ctm() {
    let root = TempDir::new();
    let out = TempDir::new();
    root.write("empty/readme.txt", "x");
    assert!(import_xmax(&root.path().join("empty"), out.path()).is_err());
    root.write("bad/bad.ctm", "<DTDExtensions>");
    assert!(import_xmax(&root.path().join("bad"), out.path()).is_err());
    assert!(import_xmax(&root.path().join("nope"), out.path()).is_err());
    root.write("notzip.xac", "plain text");
    assert!(matches!(
        import_xmax(&root.path().join("notzip.xac"), out.path()),
        Err(Error::Archive { .. })
    ));
}

fn make_xac(path: &Path, entries: &[(&str, &[u8])]) {
    let file = fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, bytes) in entries {
        zip.start_file(*name, options).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
}

#[test]
fn imports_an_xac_archive() {
    let root = TempDir::new();
    let out = TempDir::new();
    let meta = r#"<XPS_ROOT><XPS_PROPS XPS_PROPS_NAME="XAC_ASSET_PROPERTIES"><XPS_ATTR XPS_ATTR_NAME="_XAC_USE_AS_PROP" XPS_ATTR_VAL="XMETAL_CSS_DEFAULT"/><XPS_ATTR XPS_ATTR_NAME="Name" XPS_ATTR_VAL="Main.css"/></XPS_PROPS><XPS_PROPS XPS_PROPS_NAME="XAC_ASSET_PROPERTIES"><XPS_ATTR XPS_ATTR_NAME="_XAC_USE_AS_PROP" XPS_ATTR_VAL="XMETAL_CSS_STRUCT"/><XPS_ATTR XPS_ATTR_NAME="Name" XPS_ATTR_VAL="Main_structure.css"/></XPS_PROPS></XPS_ROOT>"#;
    let xac = root.path().join("Pack.xac");
    make_xac(
        &xac,
        &[
            ("Pack.ctm", CTM.as_bytes()),
            ("Main.css", b"@import url(common.css);"),
            ("Main_structure.css", b"a{}"),
            ("common.css", b"b { background: url(Dot.PNG) }"),
            ("dot.png", &[9]),
            ("unused.gif", &[1]),
            ("Pack.rld", b"compiled"),
            ("Pack.tbr", b"tbr"),
            ("meta-inf.xml", meta.as_bytes()),
            ("../escape.txt", b"nope"),
        ],
    );
    let report = import_xmax(&xac, out.path()).unwrap();
    let pkg = DoctypePackage::load(out.path()).unwrap();
    assert!(pkg.missing_files().is_empty(), "{report}");
    assert_eq!(pkg.doctype.name, "Pack");
    assert_eq!(pkg.files.style.as_deref(), Some("Main.css"));
    assert_eq!(
        pkg.files.structure_style.as_deref(),
        Some("Main_structure.css")
    );
    assert!(pkg.files.schema.is_none());
    assert!(out.path().join("common.css").is_file() && out.path().join("dot.png").is_file());
    assert!(!out.path().join("unused.gif").exists());
    assert!(!out.path().parent().unwrap().join("escape.txt").exists());
    assert!(report.ignored.iter().any(|i| i.starts_with("Pack.rld")));
    assert!(report.ignored.iter().any(|i| i.contains("1 image")));
    assert!(report.warnings.iter().any(|w| w.contains("unsafe")));
}

#[test]
fn import_all_disambiguates_and_verifies() {
    let root = TempDir::new();
    let out = TempDir::new();
    make_folder_set(&root, "Alpha");
    root.write("Other/Cust/common.css", "x{}");
    root.write("Other/Cust/Alpha/Alpha.ctm", CTM);
    root.write("Other/Cust/Alpha/Alpha.css", "a{}");
    root.write("Other/Cust/Alpha/Alpha_structure.css", "a{}");
    make_xac(
        &root.path().join("Zed.xac"),
        &[
            ("Zed.ctm", CTM.as_bytes()),
            ("Zed.css", b"a{}"),
            ("Zed_structure.css", b"a{}"),
        ],
    );

    let jobs = discover(root.path()).unwrap();
    let names: Vec<&str> = jobs.iter().map(|j| j.dir_name.as_str()).collect();
    assert_eq!(names.len(), 3);
    assert!(names.contains(&"Zed"));
    assert!(
        names.contains(&"Alpha--Cust") && names.contains(&"Alpha--Other_Cust"),
        "{names:?}"
    );

    let entries = import_all(root.path(), out.path()).unwrap();
    assert_eq!(entries.len(), 3);
    for entry in &entries {
        let report = entry
            .result
            .as_ref()
            .unwrap_or_else(|e| panic!("{}: {e}", entry.job.dir_name));
        assert_eq!(report.name, entry.job.dir_name);
        assert!(entry.out_dir.join(MANIFEST_NAME).is_file());
    }
    assert!(verify(&entries[0].out_dir).is_ok());
}

#[test]
fn import_all_reports_individual_failures() {
    let root = TempDir::new();
    let out = TempDir::new();
    root.write("Good/Good.ctm", CTM);
    root.write("Bad/Bad.ctm", "<nonsense>");
    let entries = import_all(root.path(), out.path()).unwrap();
    assert_eq!(entries.iter().filter(|e| e.result.is_ok()).count(), 1);
    assert_eq!(entries.iter().filter(|e| e.result.is_err()).count(), 1);
}
