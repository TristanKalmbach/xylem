//! Importer for XMetaL XMAX customizations (folder or `.xac` archive).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use glob::Pattern;
use xylem_dom::{Document, local_name};

use crate::behaviour::{Behaviour, ElementPrettyPrint, PrettyPrint};
use crate::css;
use crate::ctm::{self, Ctm};
use crate::error::{Error, Result};
use crate::package::{DoctypeInfo, DoctypePackage, Files, MatchRules};
use crate::source::{Loaded, Source};
use crate::template::Template;
use crate::toolbar::default_toolbar;

/// What an import did.
#[derive(Clone, Debug, Default)]
pub struct ImportReport {
    /// Package name (the `.ctm` file stem).
    pub name: String,
    pub source: PathBuf,
    pub out_dir: PathBuf,
    /// Things that were converted or copied, one line each.
    pub converted: Vec<String>,
    /// Things deliberately not converted.
    pub ignored: Vec<String>,
    /// Things that may need a human look.
    pub warnings: Vec<String>,
}

impl fmt::Display for ImportReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{} -> {}", self.source.display(), self.out_dir.display())?;
        for (title, lines) in [
            ("converted", &self.converted),
            ("ignored", &self.ignored),
            ("warnings", &self.warnings),
        ] {
            if lines.is_empty() {
                continue;
            }
            writeln!(f, "  {title}:")?;
            for line in lines {
                writeln!(f, "    {line}")?;
            }
        }
        Ok(())
    }
}

/// Converts an XMAX customization into a Xylem doctype package in `out_dir`.
///
/// `source` is a folder containing a `.ctm` file, or a `.xac` archive.
/// Existing files in `out_dir` with the same names are overwritten.
pub fn import_xmax(source: &Path, out_dir: &Path) -> Result<ImportReport> {
    let mut src = Source::open(source)?;
    let mut run = Run {
        out_dir,
        report: ImportReport {
            source: source.to_path_buf(),
            out_dir: out_dir.to_path_buf(),
            ..Default::default()
        },
        written: HashMap::new(),
        seen_css: HashSet::new(),
        used: HashSet::new(),
    };

    let ctm_name = run.pick_ctm(&src, source)?;
    let stem = file_stem(&ctm_name);
    run.report.name = stem.clone();
    let ctm_file = src
        .read(&ctm_name)?
        .ok_or_else(|| Error::invalid("ctm vanished"))?;
    let ctm =
        ctm::parse(&ctm_file.bytes).map_err(|e| Error::invalid(format!("{ctm_name}: {e}")))?;
    run.report.warnings.extend(ctm.warnings.iter().cloned());
    run.used.insert(ctm_name.to_ascii_lowercase());

    fs::create_dir_all(out_dir).map_err(|e| Error::io(out_dir, e))?;

    let mut files = Files::default();
    let (style_name, structure_name) = run.stylesheet_names(&mut src, &stem)?;
    files.style = run.import_stylesheet(&mut src, style_name.as_deref(), "style")?;
    files.structure_style =
        run.import_stylesheet(&mut src, structure_name.as_deref(), "structure style")?;

    let (schema_file, roots) = run.import_schema(&mut src, &stem)?;
    files.schema = schema_file.clone();
    files.entities = run.import_entities(&mut src)?;
    run.account_for_leftovers(&src);

    let (behaviour, pretty, descriptions, templates) = run.convert_ctm(&ctm);
    let mut schema_locations = vec![Pattern::escape(&format!("{stem}.xsd"))];
    if let Some(s) = &schema_file {
        let pattern = Pattern::escape(s);
        if !schema_locations.contains(&pattern) {
            schema_locations.push(pattern);
        }
    }
    let toolbar = default_toolbar(&behaviour);
    let mut package = DoctypePackage::new(DoctypeInfo {
        name: stem.clone(),
        description: format!("Imported from XMAX customization `{stem}`"),
        version: "1.0".into(),
    });
    package.matching = MatchRules {
        roots,
        schema_locations,
        ..MatchRules::default()
    };
    package.files = files;
    package.templates = templates;
    package.behaviour = behaviour;
    // XMAX hosts typically locked down the special processing instructions
    // that the stylesheets give a `$PROCINS[xm-pi-target=...]` rule, as well
    // as entity references. Carry that over as protected markers.
    let targets = styled_pi_targets(out_dir);
    if !targets.is_empty() {
        run.report.converted.push(format!("protected PI targets: {}", targets.join(", ")));
    }
    package.behaviour.protected_pis = targets;
    package.behaviour.protect_entity_refs = true;
    package.pretty_print = pretty;
    package.descriptions = descriptions;
    package.toolbar = toolbar;
    package.save(out_dir)?;
    run.report.converted.push("doctype.toml".into());
    Ok(run.report)
}

fn file_stem(name: &str) -> String {
    Path::new(name)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| name.to_string())
}

fn extension(name: &str) -> String {
    Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

struct Run<'a> {
    out_dir: &'a Path,
    report: ImportReport,
    /// Files written to the output, by lower-cased name.
    written: HashMap<String, Vec<u8>>,
    seen_css: HashSet<String>,
    /// Source files that were consumed or explicitly accounted for.
    used: HashSet<String>,
}

impl Run<'_> {
    fn warn(&mut self, msg: String) {
        if !self.report.warnings.contains(&msg) {
            self.report.warnings.push(msg);
        }
    }

    /// Chooses the `.ctm`: the only one, or the one named like the source.
    fn pick_ctm(&mut self, src: &Source, source: &Path) -> Result<String> {
        let mut ctms: Vec<&String> = src
            .names()
            .iter()
            .filter(|n| extension(n) == "ctm")
            .collect();
        let wanted = file_stem(
            &source
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
        );
        ctms.sort_by_key(|n| {
            (
                !file_stem(n).eq_ignore_ascii_case(&wanted),
                n.to_ascii_lowercase(),
            )
        });
        let Some(chosen) = ctms.first().map(|n| n.to_string()) else {
            return Err(Error::invalid(format!(
                "{}: no .ctm file found",
                source.display()
            )));
        };
        if ctms.len() > 1 {
            let others: Vec<&str> = ctms.iter().skip(1).map(|n| n.as_str()).collect();
            self.warn(format!(
                "several .ctm files found; imported `{chosen}`, skipped {}",
                others.join(", ")
            ));
        }
        Ok(chosen)
    }

    /// Names of the main and structure stylesheets.
    fn stylesheet_names(
        &mut self,
        src: &mut Source,
        stem: &str,
    ) -> Result<(Option<String>, Option<String>)> {
        let mut style = None;
        let mut structure = None;
        if let Some(meta) = src.read("meta-inf.xml")? {
            self.used.insert("meta-inf.xml".into());
            match asset_roles(&meta.bytes) {
                Ok(roles) => {
                    style = roles.get("XMETAL_CSS_DEFAULT").cloned();
                    structure = roles.get("XMETAL_CSS_STRUCT").cloned();
                    self.report
                        .converted
                        .push("archive metadata: stylesheet roles".into());
                }
                Err(e) => self.warn(format!("meta-inf.xml could not be read: {e}")),
            }
        }
        let exists = |n: &str| src.names().iter().any(|x| x.eq_ignore_ascii_case(n));
        let style = style
            .filter(|n| exists(n))
            .or_else(|| Some(format!("{stem}.css")).filter(|n| exists(n)));
        let structure = structure
            .filter(|n| exists(n))
            .or_else(|| Some(format!("{stem}_structure.css")).filter(|n| exists(n)));
        if style.is_none() {
            self.warn(format!("no main stylesheet `{stem}.css` found"));
        }
        if structure.is_none() {
            self.warn(format!(
                "no structure stylesheet `{stem}_structure.css` found"
            ));
        }
        Ok((style, structure))
    }

    /// Copies a primary stylesheet and everything it references; returns its output name.
    fn import_stylesheet(
        &mut self,
        src: &mut Source,
        name: Option<&str>,
        role: &str,
    ) -> Result<Option<String>> {
        let Some(name) = name else { return Ok(None) };
        let Some(first) = src.read(name)? else {
            return Ok(None);
        };
        let out_name = first.name.clone();
        self.used.insert(name.to_ascii_lowercase());

        let mut queue = VecDeque::from([(first, role.to_string())]);
        while let Some((sheet, role)) = queue.pop_front() {
            if !self.seen_css.insert(sheet.name.to_ascii_lowercase()) {
                continue;
            }
            let (mut text, lossless_latin1) = decode_text(&sheet.bytes);
            let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
            for reference in css::references(&text) {
                match src.locate(&reference.target, sheet.origin.as_deref())? {
                    Some(found) => {
                        if found.name != reference.target {
                            edits.push((reference.range.clone(), found.name.clone()));
                        }
                        if reference.is_stylesheet() {
                            queue.push_back((found, "imported stylesheet".to_string()));
                        } else {
                            self.copy_asset(found);
                        }
                    }
                    None => self.warn(format!(
                        "{}: referenced file `{}` not found",
                        sheet.name, reference.target
                    )),
                }
            }
            for (range, replacement) in edits.into_iter().rev() {
                text.replace_range(range, &replacement);
            }
            let bytes = encode_text(&text, lossless_latin1);
            self.used.insert(sheet.name.to_ascii_lowercase());
            if self.emit(&sheet.name, bytes)? {
                self.report
                    .converted
                    .push(format!("{role}: {}", sheet.name));
            }
        }
        Ok(Some(out_name))
    }

    fn copy_asset(&mut self, file: Loaded) {
        self.used.insert(file.name.to_ascii_lowercase());
        match self.emit(&file.name, file.bytes) {
            Ok(true) => self.report.converted.push(format!("asset: {}", file.name)),
            Ok(false) => {}
            Err(e) => self.warn(format!("asset `{}` could not be copied: {e}", file.name)),
        }
    }

    /// Writes `name` into the output folder. Returns false when an identical file is already there.
    fn emit(&mut self, name: &str, bytes: Vec<u8>) -> Result<bool> {
        let key = name.to_ascii_lowercase();
        if let Some(existing) = self.written.get(&key) {
            if *existing != bytes {
                self.warn(format!(
                    "`{name}` exists twice with different content; kept the first"
                ));
            }
            return Ok(false);
        }
        let path = self.out_dir.join(name);
        fs::write(&path, &bytes).map_err(|e| Error::io(&path, e))?;
        self.written.insert(key, bytes);
        Ok(true)
    }

    /// Copies the schema and infers the root elements.
    fn import_schema(
        &mut self,
        src: &mut Source,
        stem: &str,
    ) -> Result<(Option<String>, Vec<String>)> {
        let from_project = match src {
            Source::Folder(f) => f
                .xdp
                .as_ref()
                .and_then(|(_, x)| x.setting("RulesFile"))
                .map(|s| s.replace('\\', "/")),
            Source::Archive(_) => None,
        };
        let exists = |n: &str| {
            src.names()
                .iter()
                .find(|x| x.eq_ignore_ascii_case(n))
                .cloned()
        };
        let candidates = from_project
            .iter()
            .map(|p| p.rsplit('/').next().unwrap_or(p).to_string())
            .chain([format!("{stem}.xsd"), format!("{stem}.dtd")]);
        let mut chosen = candidates.filter_map(|c| exists(&c)).next();
        if chosen.is_none() {
            let schemas: Vec<&String> = src
                .names()
                .iter()
                .filter(|n| matches!(extension(n).as_str(), "xsd" | "dtd"))
                .collect();
            if schemas.len() == 1 {
                chosen = Some(schemas[0].clone());
            }
        }
        let Some(name) = chosen else {
            self.warn("no schema source (.xsd/.dtd) available; the package has no schema and no roots to match on".into());
            return Ok((None, Vec::new()));
        };
        let file = src
            .read(&name)?
            .ok_or_else(|| Error::invalid("schema vanished"))?;
        self.used.insert(name.to_ascii_lowercase());
        let mut roots = Vec::new();
        if extension(&name) == "xsd" {
            match schema_info(&file.bytes) {
                Ok(info) => {
                    self.report.converted.push(format!(
                        "schema: {name} ({} global elements, {} roots inferred)",
                        info.globals,
                        info.roots.len()
                    ));
                    roots = info.roots;
                }
                Err(e) => {
                    self.warn(format!(
                        "{name} could not be analysed ({e}); roots not inferred"
                    ));
                    self.report.converted.push(format!("schema: {name}"));
                }
            }
        } else {
            self.warn(format!("{name} is a DTD; root elements were not inferred"));
            self.report.converted.push(format!("schema: {name}"));
        }
        let out_name = file.name.clone();
        self.emit(&out_name, file.bytes)?;
        Ok((Some(out_name), roots))
    }

    fn import_entities(&mut self, src: &mut Source) -> Result<Option<String>> {
        let names: Vec<String> = src
            .names()
            .iter()
            .filter(|n| extension(n) == "ent")
            .cloned()
            .collect();
        let mut first = None;
        for name in names {
            let Some(file) = src.read(&name)? else {
                continue;
            };
            self.used.insert(name.to_ascii_lowercase());
            let out_name = file.name.clone();
            self.emit(&out_name, file.bytes)?;
            self.report.converted.push(format!("entities: {out_name}"));
            first.get_or_insert(out_name);
        }
        Ok(first)
    }

    /// Lists every source file that was not consumed, by category.
    fn account_for_leftovers(&mut self, src: &Source) {
        let mut unreferenced_images = 0;
        let mut notes = Vec::new();
        for name in src.names() {
            if self.used.contains(&name.to_ascii_lowercase()) {
                continue;
            }
            let what = match extension(name).as_str() {
                "tbr" => {
                    "toolbar definition (.tbr): not converted, Xylem builds its toolbar from the behaviour"
                }
                "js" => "macro stub (.js): not converted",
                "mcr" => "macro file (.mcr): not converted",
                "rld" => "compiled rules (.rld): not converted",
                "xdp" => "XMetaL project file (.xdp): used only to locate the schema",
                "png" | "gif" | "jpg" | "jpeg" | "bmp" | "svg" => {
                    unreferenced_images += 1;
                    continue;
                }
                "css" => "stylesheet not referenced by the main stylesheets",
                "xsd" | "dtd" => "additional schema file",
                _ => "unrecognised file",
            };
            notes.push(format!("{name}: {what}"));
        }
        self.report.ignored.extend(notes);
        if unreferenced_images > 0 {
            self.report.ignored.push(format!(
                "{unreferenced_images} image(s) not referenced by any stylesheet"
            ));
        }
        if let Source::Archive(a) = src
            && a.unsafe_entries > 0
        {
            let n = a.unsafe_entries;
            self.warn(format!(
                "{n} archive entr(ies) with unsafe paths were skipped"
            ));
        }
    }

    fn convert_ctm(
        &mut self,
        ctm: &Ctm,
    ) -> (
        Behaviour,
        PrettyPrint,
        BTreeMap<String, String>,
        BTreeMap<String, Template>,
    ) {
        let mut behaviour = Behaviour::default();
        for (name, next) in &ctm.followed_by {
            match behaviour.followed_by.get(name) {
                None => {
                    behaviour.followed_by.insert(name.clone(), next.clone());
                }
                Some(prev) if prev != next => self.warn(format!(
                    "followed-by for `{name}` is defined twice; kept `{prev}`"
                )),
                Some(_) => {}
            }
        }
        behaviour.paragraphs = dedup(ctm.paragraphs.iter().cloned());
        for list in &ctm.bulleted_lists {
            if !behaviour.bulleted_lists.iter().any(|l| l.name == list.name) {
                behaviour.bulleted_lists.push(list.clone());
            }
        }
        for list in &ctm.numbered_lists {
            if !behaviour.numbered_lists.iter().any(|l| l.name == list.name) {
                behaviour.numbered_lists.push(list.clone());
            }
        }
        for list in &ctm.definition_lists {
            if !behaviour
                .definition_lists
                .iter()
                .any(|l| l.name == list.name)
            {
                behaviour.definition_lists.push(list.clone());
            }
        }
        for (kind, element) in &ctm.toggles {
            behaviour
                .toggles
                .entry(kind.clone())
                .or_insert_with(|| element.clone());
        }
        for image in &ctm.images {
            if !behaviour.images.iter().any(|i| i.element == image.element) {
                behaviour.images.push(image.clone());
            }
        }

        let mut pretty = PrettyPrint {
            indent_amount: ctm.indent_amount,
            max_line_length: ctm.max_line_length,
            no_pretty_printing: ctm.no_pretty_printing,
            ..PrettyPrint::default()
        };
        let mut descriptions = BTreeMap::new();
        for (name, props) in &ctm.elements {
            if let Some(d) = &props.description {
                descriptions.insert(name.clone(), d.clone());
            }
            if props.pretty != ElementPrettyPrint::default() {
                pretty.elements.insert(name.clone(), props.pretty.clone());
            }
        }

        let mut templates = BTreeMap::new();
        for (name, xml) in &ctm.templates {
            let template = Template::new(xml.clone());
            if let Err(e) = template.validate() {
                self.warn(format!(
                    "template `{name}` is not well-formed XML and was skipped: {e}"
                ));
            } else if templates.contains_key(name) {
                self.warn(format!(
                    "template `{name}` is defined twice; kept the first"
                ));
            } else {
                templates.insert(name.clone(), template);
            }
        }
        let r = &mut self.report.converted;
        r.push(format!("{} element descriptions", descriptions.len()));
        r.push(format!("{} templates", templates.len()));
        r.push(format!("{} followed-by rules", behaviour.followed_by.len()));
        r.push(format!(
            "{} toggles, {} lists, {} images, {} paragraphs",
            behaviour.toggles.len(),
            behaviour.bulleted_lists.len()
                + behaviour.numbered_lists.len()
                + behaviour.definition_lists.len(),
            behaviour.images.len(),
            behaviour.paragraphs.len()
        ));
        r.push(format!(
            "{} pretty-print element rules",
            pretty.elements.len()
        ));
        (behaviour, pretty, descriptions, templates)
    }
}

fn dedup(items: impl Iterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    items.filter(|i| seen.insert(i.clone())).collect()
}

/// Decodes stylesheet bytes. Invalid UTF-8 is read as Latin-1, which maps
/// bytes to characters one to one and so round-trips exactly.
fn decode_text(bytes: &[u8]) -> (String, bool) {
    match std::str::from_utf8(bytes) {
        Ok(s) => (s.to_string(), false),
        Err(_) => (bytes.iter().map(|&b| b as char).collect(), true),
    }
}

fn encode_text(text: &str, latin1: bool) -> Vec<u8> {
    if latin1 {
        text.chars()
            .map(|c| u8::try_from(c as u32).unwrap_or(b'?'))
            .collect()
    } else {
        text.as_bytes().to_vec()
    }
}

/// Maps `_XAC_USE_AS_PROP` values (e.g. `XMETAL_CSS_DEFAULT`) to file names
/// from an archive's `meta-inf.xml`.
fn asset_roles(bytes: &[u8]) -> Result<BTreeMap<String, String>> {
    let doc = Document::from_bytes(bytes).map_err(|e| Error::xml("meta-inf.xml", e))?;
    let mut roles = BTreeMap::new();
    for props in doc.descendants(doc.root()) {
        if doc.attr(props, "XPS_PROPS_NAME") != Some("XAC_ASSET_PROPERTIES") {
            continue;
        }
        let value_of = |wanted: &str| {
            doc.element_children(props)
                .find(|&a| doc.attr(a, "XPS_ATTR_NAME") == Some(wanted))
                .and_then(|a| doc.attr(a, "XPS_ATTR_VAL"))
                .map(str::to_string)
        };
        if let (Some(role), Some(name)) = (value_of("_XAC_USE_AS_PROP"), value_of("Name")) {
            roles.entry(role).or_insert(name);
        }
    }
    Ok(roles)
}

/// Global elements of an XSD and the subset that nothing references.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct SchemaInfo {
    pub globals: usize,
    pub roots: Vec<String>,
}

/// Scans top-level `xs:element` declarations. Candidate roots are the global
/// elements never used through `ref=`; when that leaves nothing, all globals
/// are candidates.
pub(crate) fn schema_info(bytes: &[u8]) -> Result<SchemaInfo> {
    let doc = Document::from_bytes(bytes).map_err(|e| Error::xml("xsd", e))?;
    let schema = doc
        .document_element()
        .filter(|&r| doc.element(r).is_some_and(|e| e.local_name() == "schema"))
        .ok_or_else(|| Error::invalid("root element is not a schema"))?;
    let is_element = |n| doc.element(n).is_some_and(|e| e.local_name() == "element");
    let globals: BTreeSet<String> = doc
        .element_children(schema)
        .filter(|&n| is_element(n))
        .filter_map(|n| doc.attr(n, "name").map(str::to_string))
        .collect();
    let referenced: HashSet<&str> = doc
        .descendants(schema)
        .filter(|&n| is_element(n))
        .filter_map(|n| doc.attr(n, "ref"))
        .map(local_name)
        .collect();
    let mut roots: Vec<String> = globals
        .iter()
        .filter(|g| !referenced.contains(g.as_str()))
        .cloned()
        .collect();
    if roots.is_empty() {
        roots = globals.iter().cloned().collect();
    }
    Ok(SchemaInfo {
        globals: globals.len(),
        roots,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infers_roots_from_unreferenced_globals() {
        let xsd = br#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="book"><xs:complexType><xs:sequence><xs:element ref="chapter"/></xs:sequence></xs:complexType></xs:element>
  <xs:element name="chapter" type="xs:string"/>
  <xs:element name="other"/>
</xs:schema>"#;
        let info = schema_info(xsd).unwrap();
        assert_eq!(info.globals, 3);
        assert_eq!(info.roots, ["book", "other"]);
    }

    #[test]
    fn falls_back_to_all_globals() {
        let xsd = br#"<schema xmlns="http://www.w3.org/2001/XMLSchema"><element name="a"><complexType><sequence><element ref="b"/></sequence></complexType></element><element name="b"><complexType><sequence><element ref="a"/></sequence></complexType></element></schema>"#;
        assert_eq!(schema_info(xsd).unwrap().roots, ["a", "b"]);
        assert!(schema_info(b"<other/>").is_err());
    }

    #[test]
    fn latin1_round_trips() {
        let bytes = [b'a', 0xE9, b'b'];
        let (text, latin1) = decode_text(&bytes);
        assert!(latin1);
        assert_eq!(encode_text(&text, latin1), bytes);
    }
}

/// PI targets selected by `xm-pi-target="..."` in the copied stylesheets,
/// excluding the `xm-replace_text` placeholder (which typing replaces).
fn styled_pi_targets(dir: &Path) -> Vec<String> {
    let mut found = std::collections::BTreeSet::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("css")) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let mut rest = text.as_str();
        while let Some(i) = rest.find("xm-pi-target") {
            rest = &rest[i + "xm-pi-target".len()..];
            let value = rest.trim_start().strip_prefix('=').map(str::trim_start);
            if let Some(v) = value {
                if let Some(q) = v.chars().next().filter(|c| *c == '"' || *c == '\'') {
                    if let Some(end) = v[1..].find(q) {
                        let target = &v[1..1 + end];
                        if !target.is_empty() && target != "xm-replace_text" {
                            found.insert(target.to_string());
                        }
                    }
                }
            }
        }
    }
    found.into_iter().collect()
}
