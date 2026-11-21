//! File-based loading and large-document behaviour.

use std::fs;
use std::path::PathBuf;

use xylem_style::{Display, StyleEngine, Stylesheet};

/// A scratch directory removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("xylem-style-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn load_follows_imports_relative_to_the_importer_and_parent_folders() {
    let dir = TempDir::new("load");
    let sub = dir.0.join("sub");
    fs::create_dir_all(&sub).unwrap();
    fs::write(
        dir.0.join("common.css"),
        "\u{feff}@namespace url(urn:x);\n@sq-plus-minus;\npara { display: block }\n",
    )
    .unwrap();
    fs::write(sub.join("local.css"), "title { display: block }").unwrap();
    // `common.css` is only found by searching the parent folder.
    fs::write(
        sub.join("main.css"),
        "@import url(\"common.css\");\n@import 'local.css';\ntitle { color: red }\n",
    )
    .unwrap();

    let sheet = Stylesheet::load(&sub.join("main.css")).unwrap();
    assert!(sheet.warnings().is_empty(), "{:?}", sheet.warnings());
    assert!(sheet.unresolved_imports().is_empty());
    assert!(sheet.flags().plus_minus);
    assert_eq!(sheet.rule_count(), 3);

    let doc = xylem_dom::parse("<doc><para/><title/></doc>").unwrap();
    let styles = StyleEngine::new(sheet).compute(&doc);
    let find = |name: &str| {
        doc.descendants(doc.root())
            .find(|&n| doc.name(n) == Some(name))
            .unwrap()
    };
    assert_eq!(styles.get(find("para")).display, Display::Block);
    assert_eq!(styles.get(find("title")).display, Display::Block);
    assert_eq!(styles.get(find("title")).color, Some([255, 0, 0, 255]));
}

#[test]
fn load_reports_a_missing_top_level_file() {
    assert!(Stylesheet::load(&std::env::temp_dir().join("definitely-missing-xylem.css")).is_err());
}

#[test]
fn twenty_thousand_elements_compute_correctly() {
    let mut xml = String::from("<doc>");
    for i in 0..4000 {
        xml.push_str(&format!(
            "<sec id=\"s{i}\"><title>t</title><p>a</p><p>b</p><li/></sec>"
        ));
    }
    xml.push_str("</doc>");
    let doc = xylem_dom::parse(&xml).unwrap();
    let sheet = Stylesheet::parse(
        "doc { counter-reset: n } sec { display: block; counter-increment: n } sec>title { font-weight: bold } \
         sec:before { content: counter(n) } li { display: list-item } sec + sec > title:first-child { color: red }",
    );
    let styles = StyleEngine::new(sheet).compute(&doc);
    let secs: Vec<_> = doc
        .element_children(doc.document_element().unwrap())
        .collect();
    assert_eq!(secs.len(), 4000);
    assert_eq!(styles.before(secs[3999]).unwrap().text, "4000");
    let last_title = doc.element_children(secs[3999]).next().unwrap();
    assert_eq!(styles.get(last_title).color, Some([255, 0, 0, 255]));
    assert!(styles.get(last_title).bold);
    let first_title = doc.element_children(secs[0]).next().unwrap();
    assert_eq!(styles.get(first_title).color, None);
}
