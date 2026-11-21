//! A folder of modules treated as a project (a CSDB): scanning it, filtering
//! the list and checking the references between modules.
//!
//! Scanning reads every XML file under the folder once and keeps only owned
//! summaries, so the result can cross threads and outlive the documents.
//! References are checked against an index of the identifications actually
//! read, not file names, so modules with non-standard names still resolve.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering as AtomicOrdering};

use xylem_dom::{Document, NodeId};

use crate::detect::detect;
use crate::ident::{DmIdent, DmTitle, Dmc, IssueInfo, Language, PmIdent, Pmc};
use crate::issue::{Issue, Markup};
use crate::refs::{find_dm_refs, find_pm_refs};
use crate::template::{attr_value, dm_code_element, pm_code_element, text};

/// Whether a module (or a reference) is a data module or a publication module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModuleType {
    Data,
    Publication,
}

/// A reference found in a module, kept by position rather than node so it can
/// be found again in a freshly opened copy of the file.
#[derive(Debug, Clone, PartialEq)]
pub struct ModuleRef {
    pub target: ModuleType,
    /// `DMC-...` or `PMC-...`.
    pub code: String,
    pub issue_info: Option<IssueInfo>,
    pub language: Option<Language>,
    /// Index among the module's references of the same type, in document
    /// order (as returned by [`find_dm_refs`] or [`find_pm_refs`]).
    pub ordinal: usize,
    /// The reference names the module's BREX. The default S1000D BREX is
    /// usually not part of a project, so callers may want to treat these apart.
    pub in_brex: bool,
}

impl ModuleRef {
    /// Finds this reference in `doc`, a copy of the module it was read from.
    /// Returns `None` when the document has changed so that it is gone.
    pub fn locate(&self, doc: &Document) -> Option<NodeId> {
        let (node, code) = match self.target {
            ModuleType::Data => {
                let r = find_dm_refs(doc).into_iter().nth(self.ordinal)?;
                (r.node, r.dmc.to_string())
            }
            ModuleType::Publication => {
                let r = find_pm_refs(doc).into_iter().nth(self.ordinal)?;
                (r.node, r.pmc.to_string())
            }
        };
        (code.eq_ignore_ascii_case(&self.code)).then_some(node)
    }
}

/// What the project list shows about one module.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectModule {
    pub path: PathBuf,
    pub kind: ModuleType,
    /// `DMC-...` or `PMC-...`.
    pub code: String,
    pub dmc: Option<Dmc>,
    pub pmc: Option<Pmc>,
    pub issue_info: Option<IssueInfo>,
    pub language: Option<Language>,
    /// The data module title as tech name and info name.
    pub dm_title: Option<DmTitle>,
    /// One-line title (`tech name - info name`, or the publication title).
    pub title: String,
    /// The S1000D issue the module is written to, when it can be told.
    pub issue: Option<Issue>,
    pub markup: Markup,
    pub refs: Vec<ModuleRef>,
}

impl ProjectModule {
    /// Reads a module from a parsed document; `None` when it is not a DM or PM.
    pub fn from_document(doc: &Document, path: PathBuf) -> Option<ProjectModule> {
        let info = detect(doc);
        let markup = info.as_ref().map_or_else(
            || {
                if doc
                    .document_element()
                    .and_then(|r| crate::xmlutil::child(doc, r, "identAndStatusSection"))
                    .is_some()
                {
                    Markup::Modern
                } else {
                    Markup::Legacy
                }
            },
            |i| i.issue.markup(),
        );
        let refs = collect_refs(doc);
        if let Some(dm) = DmIdent::from_document(doc) {
            return Some(ProjectModule {
                path,
                kind: ModuleType::Data,
                code: dm.dmc.to_string(),
                dmc: Some(dm.dmc),
                pmc: None,
                issue_info: dm.issue_info,
                language: dm.language,
                title: dm
                    .title
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
                dm_title: dm.title,
                issue: info.map(|i| i.issue),
                markup,
                refs,
            });
        }
        let pm = PmIdent::from_document(doc)?;
        Some(ProjectModule {
            path,
            kind: ModuleType::Publication,
            code: pm.pmc.to_string(),
            dmc: None,
            pmc: Some(pm.pmc),
            issue_info: pm.issue_info,
            language: pm.language,
            dm_title: None,
            title: pm.title.unwrap_or_default(),
            issue: info.map(|i| i.issue),
            markup,
            refs,
        })
    }

    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// True when every whitespace-separated word of `query` occurs
    /// (ignoring case) in the code, title, issue, language or file name.
    pub fn matches(&self, query: &str) -> bool {
        let haystack = format!(
            "{}\n{}\n{}\n{}\n{}",
            self.code,
            self.title,
            self.issue_info
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default(),
            self.language
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default(),
            self.file_name()
        )
        .to_lowercase();
        query
            .split_whitespace()
            .all(|w| haystack.contains(&w.to_lowercase()))
    }

    /// A complete `dmRef` or `pmRef` (issue 4.0+ markup) pointing at this
    /// module, with its issue, language and title.
    pub fn reference_xml(&self) -> String {
        let issue = self
            .issue_info
            .as_ref()
            .map(|i| {
                format!(
                    r#"<issueInfo issueNumber="{}" inWork="{}"/>"#,
                    attr_value(&i.issue_number),
                    attr_value(&i.in_work)
                )
            })
            .unwrap_or_default();
        let language = self
            .language
            .as_ref()
            .map(|l| {
                format!(
                    r#"<language languageIsoCode="{}" countryIsoCode="{}"/>"#,
                    attr_value(&l.language_iso_code),
                    attr_value(&l.country_iso_code)
                )
            })
            .unwrap_or_default();
        match (self.kind, &self.dmc, &self.pmc) {
            (ModuleType::Data, Some(dmc), _) => {
                let title = self
                    .dm_title
                    .as_ref()
                    .map(|t| {
                        let info = t
                            .info_name
                            .as_deref()
                            .map(|i| format!("<infoName>{}</infoName>", text(i)))
                            .unwrap_or_default();
                        format!(
                            "<dmRefAddressItems><dmTitle><techName>{}</techName>{info}</dmTitle></dmRefAddressItems>",
                            text(&t.tech_name)
                        )
                    })
                    .unwrap_or_default();
                format!(
                    "<dmRef><dmRefIdent>{}{issue}{language}</dmRefIdent>{title}</dmRef>",
                    dm_code_element(dmc)
                )
            }
            (_, _, Some(pmc)) => {
                let title = if self.title.is_empty() {
                    String::new()
                } else {
                    format!(
                        "<pmRefAddressItems><pmTitle>{}</pmTitle></pmRefAddressItems>",
                        text(&self.title)
                    )
                };
                format!(
                    "<pmRef><pmRefIdent>{}{issue}{language}</pmRefIdent>{title}</pmRef>",
                    pm_code_element(pmc)
                )
            }
            // A data module always has its code; this arm cannot be reached
            // through `from_document`.
            _ => String::new(),
        }
    }

    /// Element name of the reference [`ProjectModule::reference_xml`] builds.
    pub fn reference_element(&self) -> &'static str {
        match self.kind {
            ModuleType::Data => "dmRef",
            ModuleType::Publication => "pmRef",
        }
    }
}

fn collect_refs(doc: &Document) -> Vec<ModuleRef> {
    let in_brex = |n: NodeId| {
        doc.ancestors(n)
            .any(|a| matches!(doc.name(a), Some("brexDmRef" | "brexref")))
    };
    let dms = find_dm_refs(doc)
        .into_iter()
        .enumerate()
        .map(|(ordinal, r)| ModuleRef {
            target: ModuleType::Data,
            code: r.dmc.to_string(),
            issue_info: r.issue_info,
            language: r.language,
            ordinal,
            in_brex: in_brex(r.node),
        });
    let pms = find_pm_refs(doc)
        .into_iter()
        .enumerate()
        .map(|(ordinal, r)| ModuleRef {
            target: ModuleType::Publication,
            code: r.pmc.to_string(),
            issue_info: r.issue_info,
            language: r.language,
            ordinal,
            in_brex: false,
        });
    dms.chain(pms).collect()
}

// ------------------------------------------------------------------ scanning

/// Progress of a running scan, shared with the thread that shows it.
#[derive(Debug, Default)]
pub struct ScanProgress {
    pub total: AtomicUsize,
    pub done: AtomicUsize,
    /// Set to stop the scan early; the partial result is returned.
    pub cancel: AtomicBool,
}

/// The modules found in a folder.
#[derive(Debug, Clone, Default)]
pub struct Project {
    pub dir: PathBuf,
    /// Sorted by code, then issue.
    pub modules: Vec<ProjectModule>,
    /// Files that look like XML but could not be read or parsed.
    pub errors: Vec<(PathBuf, String)>,
}

/// XML files under `dir`, including sub-folders (hidden folders are skipped).
pub fn xml_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(d) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if kind.is_dir() {
                if !name.starts_with('.') {
                    pending.push(path);
                }
            } else if kind.is_file()
                && Path::new(name.as_ref())
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("xml"))
            {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// Reads one file. `Ok(None)` means well-formed XML that is not a DM or PM.
pub fn scan_file(path: &Path) -> Result<Option<ProjectModule>, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let doc = Document::from_bytes(&bytes).map_err(|e| e.to_string())?;
    Ok(ProjectModule::from_document(&doc, path.to_path_buf()))
}

/// Scans every XML file under `dir`, using all available cores.
pub fn scan_dir(dir: &Path, progress: &ScanProgress) -> Project {
    let files = xml_files(dir);
    progress.total.store(files.len(), AtomicOrdering::Relaxed);
    let next = AtomicUsize::new(0);
    let workers = std::thread::available_parallelism()
        .map_or(2, |n| n.get())
        .clamp(1, 8);
    // Each worker returns (file index, outcome) pairs.
    type Scanned = (usize, Result<Option<ProjectModule>, String>);
    let results: Vec<Vec<Scanned>> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                s.spawn(|| {
                    let mut out = Vec::new();
                    while !progress.cancel.load(AtomicOrdering::Relaxed) {
                        let i = next.fetch_add(1, AtomicOrdering::Relaxed);
                        let Some(file) = files.get(i) else { break };
                        out.push((i, scan_file(file)));
                        progress.done.fetch_add(1, AtomicOrdering::Relaxed);
                    }
                    out
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or_default())
            .collect()
    });
    let mut project = Project {
        dir: dir.to_path_buf(),
        ..Default::default()
    };
    for (i, result) in results.into_iter().flatten() {
        match result {
            Ok(Some(m)) => project.modules.push(m),
            Ok(None) => {}
            Err(e) => project.errors.push((files[i].clone(), e)),
        }
    }
    project.modules.sort_by(|a, b| {
        a.code
            .cmp(&b.code)
            .then_with(|| compare_issue(a.issue_info.as_ref(), b.issue_info.as_ref()))
            .then_with(|| a.path.cmp(&b.path))
    });
    project.errors.sort();
    project
}

// ------------------------------------------------------------------ checking

/// Orders issues numerically by issue number, then in-work number.
pub fn compare_issue(a: Option<&IssueInfo>, b: Option<&IssueInfo>) -> Ordering {
    let key = |i: Option<&IssueInfo>| {
        i.map(|i| {
            (
                i.issue_number.parse::<u32>().unwrap_or(0),
                i.in_work.parse::<u32>().unwrap_or(0),
            )
        })
    };
    key(a).cmp(&key(b))
}

/// What is wrong with a module or one of its references.
#[derive(Debug, Clone, PartialEq)]
pub enum FindingKind {
    /// No module in the folder has the referenced code (and language, and
    /// issue, when the reference names them and no newer issue exists).
    Unresolved,
    /// The reference names an issue older than the newest in the folder.
    OlderIssue { newest: IssueInfo },
    /// A newer issue of this module is in the folder.
    Outdated {
        newest: IssueInfo,
        newest_path: PathBuf,
    },
}

/// One result of [`check_references`].
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    /// Index into the modules that were checked.
    pub module: usize,
    /// Index into that module's `refs`, for reference findings.
    pub reference: Option<usize>,
    pub kind: FindingKind,
    pub message: String,
}

struct Entry<'a> {
    language: Option<&'a Language>,
    issue: Option<&'a IssueInfo>,
    module: usize,
}

fn same_language(a: Option<&Language>, b: Option<&Language>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => {
            a.language_iso_code
                .eq_ignore_ascii_case(&b.language_iso_code)
                && a.country_iso_code.eq_ignore_ascii_case(&b.country_iso_code)
        }
        // A reference without a language accepts any.
        _ => true,
    }
}

/// Checks every reference of every module against the others, and finds
/// modules superseded by a newer issue in the same set.
pub fn check_references(modules: &[ProjectModule]) -> Vec<Finding> {
    let mut index: HashMap<String, Vec<Entry>> = HashMap::new();
    for (i, m) in modules.iter().enumerate() {
        index.entry(m.code.to_uppercase()).or_default().push(Entry {
            language: m.language.as_ref(),
            issue: m.issue_info.as_ref(),
            module: i,
        });
    }
    let mut findings = Vec::new();
    for (mi, m) in modules.iter().enumerate() {
        for (ri, r) in m.refs.iter().enumerate() {
            let candidates: Vec<&Entry> = index
                .get(&r.code.to_uppercase())
                .map(|v| {
                    v.iter()
                        .filter(|e| same_language(r.language.as_ref(), e.language))
                        .collect()
                })
                .unwrap_or_default();
            let newest = candidates
                .iter()
                .max_by(|a, b| compare_issue(a.issue, b.issue))
                .and_then(|e| e.issue);
            let mut push = |kind, message| {
                findings.push(Finding {
                    module: mi,
                    reference: Some(ri),
                    kind,
                    message,
                })
            };
            let what = if r.in_brex {
                "BREX reference"
            } else {
                "reference"
            };
            if candidates.is_empty() {
                push(
                    FindingKind::Unresolved,
                    format!("{what} to {} not found in the folder", describe(r)),
                );
                continue;
            }
            let (Some(wanted), Some(newest)) = (r.issue_info.as_ref(), newest) else {
                continue;
            };
            let present = candidates
                .iter()
                .any(|e| compare_issue(e.issue, Some(wanted)) == Ordering::Equal);
            match compare_issue(Some(wanted), Some(newest)) {
                Ordering::Less => push(
                    FindingKind::OlderIssue {
                        newest: newest.clone(),
                    },
                    format!(
                        "{what} to {} issue {wanted}{}; issue {newest} is in the folder",
                        r.code,
                        if present { "" } else { " (not in the folder)" }
                    ),
                ),
                _ if !present => push(
                    FindingKind::Unresolved,
                    format!(
                        "{what} to {} issue {wanted} not found; the newest in the folder is {newest}",
                        r.code
                    ),
                ),
                _ => {}
            }
        }
        if let Some(entries) = index.get(&m.code.to_uppercase())
            && let Some(newer) = entries
                .iter()
                .filter(|e| e.module != mi && same_language(m.language.as_ref(), e.language))
                .max_by(|a, b| compare_issue(a.issue, b.issue))
            && compare_issue(newer.issue, m.issue_info.as_ref()) == Ordering::Greater
            && let Some(newest) = newer.issue
        {
            findings.push(Finding {
                module: mi,
                reference: None,
                kind: FindingKind::Outdated {
                    newest: newest.clone(),
                    newest_path: modules[newer.module].path.clone(),
                },
                message: format!(
                    "outdated: issue {} is superseded by issue {newest}",
                    m.issue_info
                        .as_ref()
                        .map(ToString::to_string)
                        .unwrap_or_else(|| "?".into())
                ),
            });
        }
    }
    findings
}

fn describe(r: &ModuleRef) -> String {
    let mut s = r.code.clone();
    if let Some(i) = &r.issue_info {
        s.push_str(&format!(" issue {i}"));
    }
    if let Some(l) = &r.language {
        s.push_str(&format!(" ({l})"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dm(code: &str, issue: &str, refs: &str) -> String {
        let dmc = Dmc::parse(code).unwrap();
        let (no, iw) = issue.split_once('-').unwrap();
        format!(
            r#"<dmodule><identAndStatusSection><dmAddress><dmIdent>{}<language languageIsoCode="en" countryIsoCode="US"/><issueInfo issueNumber="{no}" inWork="{iw}"/></dmIdent><dmAddressItems><issueDate year="2026" month="01" day="01"/><dmTitle><techName>Pump {code}</techName><infoName>Description</infoName></dmTitle></dmAddressItems></dmAddress><dmStatus><brexDmRef><dmRef><dmRefIdent><dmCode modelIdentCode="S1000D" systemDiffCode="E" systemCode="04" subSystemCode="1" subSubSystemCode="0" assyCode="0301" disassyCode="00" disassyCodeVariant="A" infoCode="022" infoCodeVariant="A" itemLocationCode="D"/></dmRefIdent></dmRef></brexDmRef></dmStatus></identAndStatusSection><content><description><para>{refs}</para></description></content></dmodule>"#,
            dm_code_element(&dmc)
        )
    }

    fn dm_ref(code: &str, issue: Option<&str>) -> String {
        let issue = issue
            .map(|i| {
                let (no, iw) = i.split_once('-').unwrap();
                format!(r#"<issueInfo issueNumber="{no}" inWork="{iw}"/>"#)
            })
            .unwrap_or_default();
        format!(
            "<dmRef><dmRefIdent>{}{issue}</dmRefIdent></dmRef>",
            dm_code_element(&Dmc::parse(code).unwrap())
        )
    }

    fn module(xml: &str, name: &str) -> ProjectModule {
        let doc = Document::from_str(xml).unwrap();
        ProjectModule::from_document(&doc, PathBuf::from(name)).unwrap()
    }

    const A: &str = "DMC-XY-A-00-00-00-00A-040A-A";
    const B: &str = "DMC-XY-A-00-00-00-00A-520A-A";
    const C: &str = "DMC-XY-A-00-00-00-00A-720A-A";

    fn sample() -> Vec<ProjectModule> {
        vec![
            module(
                &dm(
                    A,
                    "001-00",
                    &format!(
                        "{}{}{}{}",
                        dm_ref(B, None),
                        dm_ref(B, Some("001-00")),
                        dm_ref(C, None),
                        dm_ref(B, Some("003-00"))
                    ),
                ),
                "a.xml",
            ),
            module(&dm(B, "001-00", ""), "b1.xml"),
            module(&dm(B, "002-00", ""), "b2.xml"),
        ]
    }

    #[test]
    fn reads_module_summary() {
        let m = &sample()[0];
        assert_eq!(m.kind, ModuleType::Data);
        assert_eq!(m.code, A);
        assert_eq!(m.issue_info, Some(IssueInfo::new("001", "00")));
        assert_eq!(m.title, format!("Pump {A} - Description"));
        // The BREX reference plus four in the content.
        assert_eq!(m.refs.len(), 5);
        assert!(m.refs[0].in_brex);
        assert_eq!(m.refs[2].ordinal, 2);
    }

    #[test]
    fn filter_words_match_any_field() {
        let m = &sample()[1];
        assert!(m.matches("520a"));
        assert!(m.matches("pump 001-00"));
        assert!(m.matches("en-us b1.XML"));
        assert!(!m.matches("pump 002"));
        assert!(m.matches(""));
    }

    #[test]
    fn finds_broken_stale_and_outdated() {
        let modules = sample();
        let found = check_references(&modules);
        let summary: Vec<(usize, Option<usize>, &FindingKind)> = found
            .iter()
            .map(|f| (f.module, f.reference, &f.kind))
            .collect();
        let older = FindingKind::OlderIssue {
            newest: IssueInfo::new("002", "00"),
        };
        assert_eq!(
            summary,
            [
                // The default S1000D BREX is not in the folder.
                (0, Some(0), &FindingKind::Unresolved),
                // B without an issue resolves to the latest: fine.
                // B issue 001 exists but 002 is newer.
                (0, Some(2), &older),
                // C does not exist at all.
                (0, Some(3), &FindingKind::Unresolved),
                // B issue 003 is newer than anything present.
                (0, Some(4), &FindingKind::Unresolved),
                (1, Some(0), &FindingKind::Unresolved),
                (
                    1,
                    None,
                    &FindingKind::Outdated {
                        newest: IssueInfo::new("002", "00"),
                        newest_path: PathBuf::from("b2.xml")
                    }
                ),
                (2, Some(0), &FindingKind::Unresolved),
            ]
        );
        assert!(found[0].message.starts_with("BREX reference"));
    }

    #[test]
    fn missing_old_issue_counts_as_stale() {
        let modules = vec![
            module(&dm(A, "001-00", &dm_ref(B, Some("001-00"))), "a.xml"),
            module(&dm(B, "002-01", ""), "b.xml"),
        ];
        let found = check_references(&modules);
        let content: Vec<&Finding> = found.iter().filter(|f| f.reference == Some(1)).collect();
        assert_eq!(content.len(), 1);
        assert_eq!(
            content[0].kind,
            FindingKind::OlderIssue {
                newest: IssueInfo::new("002", "01")
            }
        );
        assert!(content[0].message.contains("not in the folder"));
    }

    #[test]
    fn language_must_match_when_stated() {
        let fr = dm(B, "001-00", "").replace(
            r#"languageIsoCode="en" countryIsoCode="US""#,
            r#"languageIsoCode="fr" countryIsoCode="FR""#,
        );
        let reference = format!(
            "<dmRef><dmRefIdent>{}<language languageIsoCode=\"en\" countryIsoCode=\"US\"/></dmRefIdent></dmRef>",
            dm_code_element(&Dmc::parse(B).unwrap())
        );
        let modules = vec![
            module(&dm(A, "001-00", &reference), "a.xml"),
            module(&fr, "b.xml"),
        ];
        let found = check_references(&modules);
        assert!(
            found
                .iter()
                .any(|f| f.reference == Some(1) && f.kind == FindingKind::Unresolved)
        );
    }

    #[test]
    fn reference_markup_round_trips() {
        let m = &sample()[1];
        let xml = m.reference_xml();
        assert!(xml.starts_with("<dmRef><dmRefIdent><dmCode "));
        assert!(xml.contains(r#"<issueInfo issueNumber="001" inWork="00"/><language languageIsoCode="en" countryIsoCode="US"/></dmRefIdent>"#));
        assert!(xml.contains("<dmRefAddressItems><dmTitle><techName>Pump"));
        let doc = Document::from_str(&xml).unwrap();
        let r = &find_dm_refs(&doc)[0];
        assert_eq!(r.dmc.to_string(), B);
        assert_eq!(r.issue_info, m.issue_info);

        let pm_xml = r#"<pm><identAndStatusSection><pmAddress><pmIdent><pmCode modelIdentCode="XY" pmIssuer="00000" pmNumber="00001" pmVolume="00"/><language languageIsoCode="en" countryIsoCode="US"/><issueInfo issueNumber="002" inWork="00"/></pmIdent><pmAddressItems><issueDate year="2026" month="01" day="01"/><pmTitle>Lift &amp; hoist</pmTitle></pmAddressItems></pmAddress></identAndStatusSection><content/></pm>"#;
        let pm = module(pm_xml, "pm.xml");
        assert_eq!(pm.kind, ModuleType::Publication);
        assert_eq!(pm.reference_element(), "pmRef");
        let xml = pm.reference_xml();
        assert!(xml.contains("<pmTitle>Lift &amp; hoist</pmTitle>"));
        let doc = Document::from_str(&xml).unwrap();
        assert_eq!(
            find_pm_refs(&doc)[0].pmc.to_string(),
            "PMC-XY-00000-00001-00"
        );
    }

    #[test]
    fn located_again_after_reparse() {
        let xml = dm(
            A,
            "001-00",
            &format!("{}{}", dm_ref(B, None), dm_ref(C, None)),
        );
        let m = module(&xml, "a.xml");
        let doc = Document::from_str(&xml).unwrap();
        let node = m.refs[2].locate(&doc).unwrap();
        assert_eq!(doc.name(node), Some("dmRef"));
        assert_eq!(find_dm_refs(&doc)[2].dmc.to_string(), C);
    }

    #[test]
    fn scans_a_folder() {
        let dir = std::env::temp_dir().join(format!("xylem-project-{}", std::process::id()));
        let sub = dir.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(dir.join("a.XML"), dm(A, "001-00", "")).unwrap();
        std::fs::write(sub.join("b.xml"), dm(B, "001-00", "")).unwrap();
        std::fs::write(dir.join("other.xml"), "<doc/>").unwrap();
        std::fs::write(dir.join("broken.xml"), "<doc>").unwrap();
        std::fs::write(dir.join("notes.txt"), "x").unwrap();
        let progress = ScanProgress::default();
        let p = scan_dir(&dir, &progress);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(progress.total.load(AtomicOrdering::Relaxed), 4);
        assert_eq!(progress.done.load(AtomicOrdering::Relaxed), 4);
        let codes: Vec<&str> = p.modules.iter().map(|m| m.code.as_str()).collect();
        assert_eq!(codes, [A, B]);
        assert_eq!(p.errors.len(), 1);
        assert!(p.errors[0].0.ends_with("broken.xml"));
    }
}
