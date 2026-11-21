//! Reader for XMetaL `.ctm` customization files (`<DTDExtensions>`).

use std::collections::BTreeMap;

use xylem_dom::{Document, NodeId};

use crate::behaviour::{DefinitionListDef, ElementPrettyPrint, ImageDef, ListDef};
use crate::error::{Error, Result};

/// Everything the importer understands in a `.ctm` file.
#[derive(Debug, Default, PartialEq)]
pub struct Ctm {
    pub indent_amount: Option<u32>,
    pub max_line_length: Option<u32>,
    pub no_pretty_printing: bool,
    pub elements: BTreeMap<String, ElementProps>,
    /// Template name and its snippet, in file order.
    pub templates: Vec<(String, String)>,
    pub followed_by: Vec<(String, String)>,
    pub images: Vec<ImageDef>,
    pub paragraphs: Vec<String>,
    pub bulleted_lists: Vec<ListDef>,
    pub numbered_lists: Vec<ListDef>,
    pub definition_lists: Vec<DefinitionListDef>,
    /// Lower-cased toggle kind (`bold`, `italic`, ...) and element.
    pub toggles: Vec<(String, String)>,
    /// Constructs that were present but are not converted.
    pub warnings: Vec<String>,
}

/// `<ElementProperties>` for one element.
#[derive(Debug, Default, PartialEq)]
pub struct ElementProps {
    pub description: Option<String>,
    pub pretty: ElementPrettyPrint,
}

/// Top-level sections that are known and either converted or deliberately empty.
const KNOWN_SECTIONS: &[&str] = &[
    "NSPrefixMap",
    "ElementPropertiesList",
    "Templates",
    "FollowedByList",
    "Images",
    "Paragraphs",
    "ListElements",
    "TogglingInlines",
];

/// Parses CTM bytes (BOM and encoding are handled by the DOM layer).
pub fn parse(bytes: &[u8]) -> Result<Ctm> {
    let doc = Document::from_bytes(bytes).map_err(|e| Error::xml("ctm", e))?;
    let root = doc
        .document_element()
        .filter(|&r| doc.name(r) == Some("DTDExtensions"))
        .ok_or_else(|| Error::invalid("ctm: root element is not <DTDExtensions>"))?;
    let mut reader = Reader {
        doc: &doc,
        ctm: Ctm::default(),
        prefixes: Vec::new(),
    };
    reader.run(root);
    Ok(reader.ctm)
}

struct Reader<'a> {
    doc: &'a Document,
    ctm: Ctm,
    /// `(ns uri, prefix)` from `<NSPrefixMap>`.
    prefixes: Vec<(String, String)>,
}

impl Reader<'_> {
    fn run(&mut self, root: NodeId) {
        // The prefix map must be read first: element names depend on it.
        for section in self.children(root, "NSPrefixMap") {
            for pair in self.children(section, "NSPrefixPair") {
                if let (Some(p), Some(ns)) =
                    (self.doc.attr(pair, "prefix"), self.doc.attr(pair, "ns"))
                {
                    self.prefixes.push((ns.to_string(), p.to_string()));
                }
            }
        }
        for section in self.doc.element_children(root).collect::<Vec<_>>() {
            let name = self.doc.name(section).unwrap_or_default().to_string();
            match name.as_str() {
                "ElementPropertiesList" => self.element_properties(section),
                "Templates" => self.templates(section),
                "FollowedByList" => self.followed_by(section),
                "Images" => self.images(section),
                "Paragraphs" => self.paragraphs(section),
                "ListElements" => self.lists(section),
                "TogglingInlines" => self.toggles(section),
                _ if KNOWN_SECTIONS.contains(&name.as_str()) => {}
                _ if self.doc.element_children(section).next().is_none() => {}
                _ => self.ctm.warnings.push(format!(
                    "ctm: section <{name}> is not supported and was skipped"
                )),
            }
        }
    }

    fn children(&self, parent: NodeId, name: &str) -> Vec<NodeId> {
        self.doc
            .element_children(parent)
            .filter(|&c| self.doc.name(c) == Some(name))
            .collect()
    }

    fn child(&self, parent: NodeId, name: &str) -> Option<NodeId> {
        self.doc
            .element_children(parent)
            .find(|&c| self.doc.name(c) == Some(name))
    }

    fn child_text(&self, parent: NodeId, name: &str) -> Option<String> {
        let text = self.doc.text_content(self.child(parent, name)?);
        let text = text.trim();
        (!text.is_empty()).then(|| text.to_string())
    }

    /// Text of a `<Name ns="...">`-style child, prefixed when it has a namespace.
    fn qualified(&self, parent: NodeId, name: &str) -> Option<String> {
        let node = self.child(parent, name)?;
        let local = self.doc.text_content(node).trim().to_string();
        if local.is_empty() {
            return None;
        }
        let prefix = self
            .doc
            .attr(node, "ns")
            .filter(|ns| !ns.is_empty())
            .and_then(|ns| self.prefixes.iter().find(|(uri, _)| uri == ns))
            .map(|(_, p)| p.as_str())
            .filter(|p| !p.is_empty());
        Some(match prefix {
            Some(p) => format!("{p}:{local}"),
            None => local,
        })
    }

    fn element_properties(&mut self, list: NodeId) {
        for node in self.doc.element_children(list).collect::<Vec<_>>() {
            match self.doc.name(node) {
                Some("GlobalPrettyPrintOptions") => self.global_pretty(node),
                Some("ElementProperties") => {
                    let Some(name) = self.qualified(node, "Name") else {
                        continue;
                    };
                    let mut props = ElementProps {
                        description: self.child_text(node, "LongDescription"),
                        ..Default::default()
                    };
                    if let Some(opts) = self.child(node, "PrettyPrintOptions") {
                        props.pretty.indent_content = self.child(opts, "IndentContent").is_some();
                        props.pretty.newline_before_start_tag =
                            self.child(opts, "NewLineBeforeStartTag").is_some();
                        props.pretty.newline_before_end_tag =
                            self.child(opts, "NewLineBeforeEndTag").is_some();
                    }
                    props.pretty.tag_color = self.child_text(node, "TagColor");
                    // Later duplicates only fill gaps, they never erase information.
                    let entry = self.ctm.elements.entry(name).or_default();
                    if entry.description.is_none() {
                        entry.description = props.description;
                    }
                    if entry.pretty.is_default() {
                        entry.pretty = props.pretty;
                    }
                }
                _ => {}
            }
        }
    }

    /// Files repeat hundreds of empty `<GlobalPrettyPrintOptions>`; only the
    /// values that are present count, and empties never reset anything.
    fn global_pretty(&mut self, node: NodeId) {
        let number = |text: Option<String>| text.and_then(|t| t.parse::<u32>().ok());
        if let Some(n) = number(self.child_text(node, "IndentAmount")) {
            self.ctm.indent_amount = Some(n);
        }
        if let Some(n) = number(self.child_text(node, "MaxLineLength")) {
            self.ctm.max_line_length = Some(n);
        }
        if self.child(node, "NoPrettyPrinting").is_some() {
            self.ctm.no_pretty_printing = true;
        }
    }

    fn templates(&mut self, list: NodeId) {
        for t in self.children(list, "Template") {
            let (Some(name), Some(mini)) =
                (self.qualified(t, "Name"), self.child(t, "MiniTemplate"))
            else {
                continue;
            };
            let xml = self.doc.text_content(mini).trim().to_string();
            self.ctm.templates.push((name, xml));
        }
    }

    fn followed_by(&mut self, list: NodeId) {
        for item in self.children(list, "FollowedByListItem") {
            if let (Some(name), Some(next)) = (
                self.qualified(item, "Name"),
                self.qualified(item, "FollowedBy"),
            ) {
                self.ctm.followed_by.push((name, next));
            }
        }
    }

    fn images(&mut self, list: NodeId) {
        for image in self.children(list, "Image") {
            let Some(element) = self.qualified(image, "Name") else {
                continue;
            };
            let Some(source_attr) = self.child_text(image, "Source-Attribute") else {
                self.ctm.warnings.push(format!(
                    "ctm: image `{element}` has no Source-Attribute and was skipped"
                ));
                continue;
            };
            self.ctm.images.push(ImageDef {
                element,
                source_attr,
                width_attr: self.child_text(image, "Width-Attribute"),
                height_attr: self.child_text(image, "Height-Attribute"),
            });
        }
    }

    fn paragraphs(&mut self, list: NodeId) {
        for p in self.children(list, "Paragraph") {
            if let Some(name) = self.qualified(p, "Name") {
                self.ctm.paragraphs.push(name);
            }
        }
    }

    fn lists(&mut self, list: NodeId) {
        for node in self.doc.element_children(list).collect::<Vec<_>>() {
            let kind = self.doc.name(node).unwrap_or_default().to_string();
            let Some(name) = self.qualified(node, "Name") else {
                continue;
            };
            let header = self.qualified(node, "ListHeader");
            match kind.as_str() {
                "BulletedList" | "NumberedList" => {
                    let Some(item) = self.qualified(node, "ListItem") else {
                        self.ctm.warnings.push(format!(
                            "ctm: list `{name}` has no ListItem and was skipped"
                        ));
                        continue;
                    };
                    let def = ListDef { name, header, item };
                    if kind == "BulletedList" {
                        self.ctm.bulleted_lists.push(def)
                    } else {
                        self.ctm.numbered_lists.push(def)
                    }
                }
                "DefinitionList" => {
                    let (Some(term), Some(definition)) = (
                        self.qualified(node, "DefinitionTerm"),
                        self.qualified(node, "DefinitionDefinition"),
                    ) else {
                        self.ctm.warnings.push(format!(
                            "ctm: definition list `{name}` is incomplete and was skipped"
                        ));
                        continue;
                    };
                    self.ctm.definition_lists.push(DefinitionListDef {
                        name,
                        header,
                        term,
                        definition,
                    });
                }
                other => self.ctm.warnings.push(format!(
                    "ctm: list kind <{other}> is not supported and was skipped"
                )),
            }
        }
    }

    fn toggles(&mut self, list: NodeId) {
        for node in self.doc.element_children(list).collect::<Vec<_>>() {
            let kind = self.doc.name(node).unwrap_or_default().to_ascii_lowercase();
            if let Some(name) = self.qualified(node, "Name") {
                self.ctm.toggles.push((kind, name));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0"?>
<!DOCTYPE DTDExtensions SYSTEM "ctm.dtd">
<DTDExtensions Version="2.0">
  <NSPrefixMap><NSPrefixPair prefix="s" ns="urn:s"></NSPrefixPair></NSPrefixMap>
  <ElementPropertiesList>
    <GlobalPrettyPrintOptions><NoPrettyPrinting/><IndentAmount>2</IndentAmount><MaxLineLength>80</MaxLineLength></GlobalPrettyPrintOptions>
    <ElementProperties>
      <Name ns="">para</Name><LongDescription>Para</LongDescription>
      <PrettyPrintOptions><IndentContent/><NewLineBeforeEndTag/></PrettyPrintOptions>
    </ElementProperties>
    <ElementProperties><Name ns="urn:s">box</Name><TagColor>#ff0000</TagColor><PrettyPrintOptions></PrettyPrintOptions></ElementProperties>
    <GlobalPrettyPrintOptions></GlobalPrettyPrintOptions>
  </ElementPropertiesList>
  <Templates>
    <Template><Name ns="">para</Name><MiniTemplate><![CDATA[<para><?xm-replace_text Type new para here ?></para>]]></MiniTemplate></Template>
  </Templates>
  <Images><Image><Name ns="">graphic</Name><Source-Attribute>contentId</Source-Attribute><Width-Attribute>w</Width-Attribute></Image></Images>
  <FollowedByList><FollowedByListItem><Name ns="">title</Name><FollowedBy ns="">para</FollowedBy></FollowedByListItem></FollowedByList>
  <Paragraphs><Paragraph><Name ns="">para</Name></Paragraph></Paragraphs>
  <ListElements>
    <BulletedList><Name ns="">bl</Name><ListHeader ns="">title</ListHeader><ListItem ns="">li</ListItem></BulletedList>
    <NumberedList><Name ns="">nl</Name><ListItem ns="">li</ListItem></NumberedList>
    <DefinitionList><Name ns="">dl</Name><DefinitionTerm ns="">term</DefinitionTerm><DefinitionDefinition ns="">def</DefinitionDefinition></DefinitionList>
    <BulletedList><Name ns="">broken</Name></BulletedList>
  </ListElements>
  <TogglingInlines><Italic><Name ns="">emphasis</Name></Italic><Bold><Name ns="">strong</Name></Bold></TogglingInlines>
  <ChangeLists></ChangeLists>
  <Mystery><X/></Mystery>
</DTDExtensions>"#;

    #[test]
    fn parses_sample() {
        let ctm = parse(SAMPLE.as_bytes()).unwrap();
        assert_eq!(
            (
                ctm.indent_amount,
                ctm.max_line_length,
                ctm.no_pretty_printing
            ),
            (Some(2), Some(80), true)
        );
        let para = &ctm.elements["para"];
        assert_eq!(para.description.as_deref(), Some("Para"));
        assert!(
            para.pretty.indent_content
                && para.pretty.newline_before_end_tag
                && !para.pretty.newline_before_start_tag
        );
        assert_eq!(
            ctm.elements["s:box"].pretty.tag_color.as_deref(),
            Some("#ff0000")
        );
        assert_eq!(
            ctm.templates,
            vec![(
                "para".to_string(),
                "<para><?xm-replace_text Type new para here ?></para>".to_string()
            )]
        );
        assert_eq!(ctm.images[0].width_attr.as_deref(), Some("w"));
        assert_eq!(ctm.images[0].height_attr, None);
        assert_eq!(
            ctm.followed_by,
            vec![("title".to_string(), "para".to_string())]
        );
        assert_eq!(ctm.paragraphs, ["para"]);
        assert_eq!(
            ctm.bulleted_lists,
            vec![ListDef {
                name: "bl".into(),
                header: Some("title".into()),
                item: "li".into()
            }]
        );
        assert_eq!(ctm.numbered_lists.len(), 1);
        assert_eq!(ctm.definition_lists[0].term, "term");
        assert_eq!(
            ctm.toggles,
            vec![
                ("italic".to_string(), "emphasis".to_string()),
                ("bold".to_string(), "strong".to_string())
            ]
        );
        assert_eq!(ctm.warnings.len(), 2, "{:?}", ctm.warnings);
        assert!(ctm.warnings.iter().any(|w| w.contains("Mystery")));
    }

    #[test]
    fn rejects_other_roots() {
        assert!(parse(b"<other/>").is_err());
        assert!(parse(b"<DTDExtensions>").is_err());
    }
}
