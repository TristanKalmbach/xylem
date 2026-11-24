//! Reading the suites' catalog and test-set files.

use std::path::{Path, PathBuf};
use xylem_dom::{Document, NodeId, NodeKind};

/// A parsed catalog or test-set file.
pub struct Xml {
    pub doc: Document,
}

impl Xml {
    pub fn load(path: &Path) -> Result<Xml, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let doc = Document::from_bytes(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Xml { doc })
    }

    pub fn root(&self) -> NodeId {
        self.doc.document_element().unwrap_or(self.doc.root())
    }

    /// Element children of `n` with local name `local`.
    pub fn kids(&self, n: NodeId, local: &str) -> Vec<NodeId> {
        self.doc
            .element_children(n)
            .filter(|&c| self.doc.element(c).is_some_and(|e| e.local_name() == local))
            .collect()
    }

    pub fn kid(&self, n: NodeId, local: &str) -> Option<NodeId> {
        self.kids(n, local).into_iter().next()
    }

    pub fn all_kids(&self, n: NodeId) -> Vec<NodeId> {
        self.doc.element_children(n).collect()
    }

    pub fn local(&self, n: NodeId) -> &str {
        self.doc.element(n).map(|e| e.local_name()).unwrap_or("")
    }

    pub fn attr(&self, n: NodeId, name: &str) -> Option<&str> {
        self.doc.attr(n, name)
    }

    /// Text content, including CDATA.
    pub fn text(&self, n: NodeId) -> String {
        let mut s = String::new();
        for &c in self.doc.children(n) {
            match self.doc.kind(c) {
                NodeKind::Text(t) | NodeKind::CData(t) => s.push_str(t),
                NodeKind::Element(_) => s.push_str(&self.text(c)),
                _ => {}
            }
        }
        s
    }

    /// The namespace bound to `prefix` at `n` ("" for the default).
    pub fn resolve(&self, n: NodeId, prefix: &str) -> Option<String> {
        let attr = if prefix.is_empty() {
            "xmlns".to_string()
        } else {
            format!("xmlns:{prefix}")
        };
        let mut cur = Some(n);
        while let Some(c) = cur {
            if let Some(v) = self.doc.attr(c, &attr) {
                return Some(v.to_string());
            }
            cur = self.doc.parent(c);
        }
        None
    }

    /// Namespace declarations in scope at `n`, innermost first.
    pub fn in_scope(&self, n: NodeId) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::new();
        let mut cur = Some(n);
        while let Some(c) = cur {
            if let Some(e) = self.doc.element(c) {
                for a in &e.attrs {
                    if let Some(p) = a.name.strip_prefix("xmlns:")
                        && !out.iter().any(|(q, _)| q == p)
                    {
                        out.push((p.to_string(), a.value.clone()));
                    }
                }
            }
            cur = self.doc.parent(c);
        }
        out
    }
}

/// The test sets a suite's `catalog.xml` lists: (name, file).
pub fn test_sets(dir: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let x = Xml::load(&dir.join("catalog.xml"))?;
    let mut v = Vec::new();
    for s in x.kids(x.root(), "test-set") {
        if let (Some(name), Some(file)) = (x.attr(s, "name"), x.attr(s, "file")) {
            v.push((name.to_string(), dir.join(file)));
        }
    }
    Ok(v)
}
