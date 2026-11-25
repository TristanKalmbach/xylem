//! Writing a result tree as text.
//!
//! [`host_style`] writes what the reference publishing host writes: the
//! `OuterXml` of a .NET `XmlDocument` loaded from the result, so empty
//! elements are `<a />`, `xsl:output` has no effect and there is no XML
//! declaration. Namespace declarations are written where a binding first
//! appears or changes: before the attributes for XSLT 2.0 results, after
//! them for XSLT 1.0 results (see [`node`]).

use crate::tree::{Kind, Tree, XML_NS};
use std::rc::Rc;

pub fn host_style(t: &Tree) -> String {
    let mut out = String::new();
    let mut scope: Vec<(Rc<str>, Rc<str>)> = vec![("".into(), "".into())];
    for &c in &t.node(0).children {
        node(t, c, &mut scope, &mut out);
    }
    out
}

fn bound<'a>(scope: &'a [(Rc<str>, Rc<str>)], p: &str) -> Option<&'a str> {
    scope
        .iter()
        .rev()
        .find(|(q, _)| &**q == p)
        .map(|(_, u)| &**u)
}

/// One node written as XML, with the namespace declarations it needs.
pub fn node_xml(t: &Tree, i: u32) -> String {
    let mut out = String::new();
    let mut scope: Vec<(Rc<str>, Rc<str>)> = vec![("".into(), "".into())];
    match t.node(i).kind {
        Kind::Attribute => {
            let n = t.node(i);
            out.push_str(&n.name.as_ref().unwrap().lexical());
            out.push_str("=\"");
            escape(&n.value, true, &mut out);
            out.push('"');
        }
        Kind::Document => {
            for &c in &t.node(i).children {
                node(t, c, &mut scope, &mut out);
            }
        }
        _ => node(t, i, &mut scope, &mut out),
    }
    out
}

fn node(t: &Tree, i: u32, scope: &mut Vec<(Rc<str>, Rc<str>)>, out: &mut String) {
    let n = t.node(i);
    match n.kind {
        Kind::Text => escape(&n.value, false, out),
        Kind::Comment => {
            out.push_str("<!--");
            out.push_str(&n.value);
            out.push_str("-->");
        }
        Kind::Pi => {
            out.push_str("<?");
            out.push_str(&n.name.as_ref().unwrap().local);
            if !n.value.is_empty() {
                out.push(' ');
                out.push_str(&n.value);
            }
            out.push_str("?>");
        }
        Kind::Element => {
            let name = n.name.as_ref().unwrap();
            let depth = scope.len();
            let mut decls: Vec<(Rc<str>, Rc<str>)> = Vec::new();
            let need = |p: &Rc<str>,
                        u: &Rc<str>,
                        scope: &Vec<(Rc<str>, Rc<str>)>,
                        decls: &mut Vec<(Rc<str>, Rc<str>)>| {
                if &**p == "xml" || (u.is_empty() && !p.is_empty()) {
                    return;
                }
                let cur = decls
                    .iter()
                    .rev()
                    .find(|(q, _)| q == p)
                    .map(|(_, u)| &**u)
                    .or(bound(scope, p));
                if cur != Some(&**u) {
                    decls.retain(|(q, _)| q != p);
                    decls.push((p.clone(), u.clone()));
                }
            };
            need(&name.prefix, &name.uri, scope, &mut decls);
            for (p, u) in &n.ns {
                need(p, u, scope, &mut decls);
            }
            for &a in &n.attrs {
                let q = t.node(a).name.as_ref().unwrap();
                if !q.prefix.is_empty() && &*q.uri != XML_NS {
                    need(&q.prefix, &q.uri, scope, &mut decls);
                }
            }
            out.push('<');
            out.push_str(&name.lexical());
            // The XSLT 1.0 processor builds the result through an XmlWriter,
            // which adds an element's namespace declarations after its
            // attributes; the XSLT 2.0 path writes them first.
            let write_decls = |out: &mut String| {
                for (p, u) in &decls {
                    if p.is_empty() {
                        out.push_str(" xmlns=\"");
                    } else {
                        out.push_str(" xmlns:");
                        out.push_str(p);
                        out.push_str("=\"");
                    }
                    escape(u, true, out);
                    out.push('"');
                }
            };
            let decls_last = crate::value::compat();
            if !decls_last {
                write_decls(out);
            }
            for &a in &n.attrs {
                let an = t.node(a);
                out.push(' ');
                out.push_str(&an.name.as_ref().unwrap().lexical());
                out.push_str("=\"");
                escape(&an.value, true, out);
                out.push('"');
            }
            if decls_last {
                write_decls(out);
            }
            scope.extend(decls);
            if n.children.is_empty() {
                out.push_str(" />");
            } else {
                out.push('>');
                for &c in &n.children {
                    node(t, c, scope, out);
                }
                out.push_str("</");
                out.push_str(&name.lexical());
                out.push('>');
            }
            scope.truncate(depth);
        }
        Kind::Document | Kind::Attribute => {}
    }
}

fn escape(s: &str, attr: bool, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attr => out.push_str("&quot;"),
            '\n' if attr => out.push_str("&#xA;"),
            '\r' => out.push_str("&#xD;"),
            '\t' if attr => out.push_str("&#x9;"),
            c => out.push(c),
        }
    }
}

// ---- xsl:output ---------------------------------------------------------

/// Elements written without an end tag by the HTML output method.
const VOID: &[&str] = &[
    "area", "base", "basefont", "br", "col", "frame", "hr", "img", "input", "isindex", "link",
    "meta", "param",
];

/// Attributes whose values are URIs (non-ASCII characters are %-escaped).
const URI_ATTRS: &[&str] = &[
    "action",
    "archive",
    "background",
    "cite",
    "classid",
    "codebase",
    "data",
    "datasrc",
    "for",
    "href",
    "longdesc",
    "profile",
    "src",
    "usemap",
];

/// Writes a result as its `xsl:output` asks, the way the reference host's
/// second step (.NET's XSLT 1.0 processor writing to a string) does.
///
/// Observed details (spec note 11): the HTML method writes `<!DOCTYPE` with
/// the root element's name, inserts a `META` content-type tag (charset
/// `utf-16`, the host writing to a string) as the first child of `head`,
/// leaves void elements unclosed, and writes line feeds in text as CR LF.
pub fn with_output(t: &Tree, o: &crate::compile::OutputSpec) -> String {
    let root = t
        .node(0)
        .children
        .iter()
        .copied()
        .find(|&c| t.node(c).kind == Kind::Element);
    let method = o.method.clone().unwrap_or_else(|| {
        let html = root
            .and_then(|r| t.node(r).name.as_ref())
            .is_some_and(|q| q.uri.is_empty() && q.local.eq_ignore_ascii_case("html"));
        if html { "html".into() } else { "xml".into() }
    });
    let mut out = String::new();
    match method.as_str() {
        "text" => {
            out = t.string_value(0);
        }
        "html" => {
            if let Some(r) = root {
                let name = t.node(r).name.as_ref().unwrap().lexical();
                match (&o.doctype_public, &o.doctype_system) {
                    (Some(p), Some(s)) => {
                        out.push_str(&format!("<!DOCTYPE {name} PUBLIC \"{p}\" \"{s}\">"))
                    }
                    (Some(p), None) => out.push_str(&format!("<!DOCTYPE {name} PUBLIC \"{p}\">")),
                    (None, Some(s)) => out.push_str(&format!("<!DOCTYPE {name} SYSTEM \"{s}\">")),
                    _ => {}
                }
            }
            let mut scope: Vec<(Rc<str>, Rc<str>)> = vec![("".into(), "".into())];
            for &c in &t.node(0).children {
                html_node(t, c, &mut scope, &mut out, false);
            }
            return out;
        }
        _ => {
            if !o.omit_declaration {
                out.push_str("<?xml version=\"1.0\" encoding=\"utf-16\"?>");
            }
            if let (Some(r), Some(s)) = (root, &o.doctype_system) {
                let name = t.node(r).name.as_ref().unwrap().lexical();
                match &o.doctype_public {
                    Some(p) => out.push_str(&format!("<!DOCTYPE {name} PUBLIC \"{p}\" \"{s}\">")),
                    None => out.push_str(&format!("<!DOCTYPE {name} SYSTEM \"{s}\">")),
                }
            }
            let mut scope: Vec<(Rc<str>, Rc<str>)> = vec![("".into(), "".into())];
            for &c in &t.node(0).children {
                node(t, c, &mut scope, &mut out);
            }
        }
    }
    out.replace('\n', "\r\n")
}

fn html_node(t: &Tree, i: u32, scope: &mut Vec<(Rc<str>, Rc<str>)>, out: &mut String, raw: bool) {
    let n = t.node(i);
    match n.kind {
        Kind::Text => {
            if raw {
                out.push_str(&n.value);
            } else {
                html_escape_text(&n.value, out);
            }
        }
        Kind::Comment => {
            out.push_str("<!--");
            out.push_str(&n.value);
            out.push_str("-->");
        }
        Kind::Pi => {
            out.push_str("<?");
            out.push_str(&n.name.as_ref().unwrap().local);
            if !n.value.is_empty() {
                out.push(' ');
                out.push_str(&n.value);
            }
            out.push('>');
        }
        Kind::Element => {
            let name = n.name.as_ref().unwrap();
            if !name.uri.is_empty() {
                // Elements in a namespace are written as XML.
                node(t, i, scope, out);
                return;
            }
            let depth = scope.len();
            out.push('<');
            out.push_str(&name.lexical());
            for (p, u) in &n.ns {
                if p.is_empty() || bound(scope, p) == Some(&**u) {
                    continue;
                }
                out.push_str(" xmlns:");
                out.push_str(p);
                out.push_str("=\"");
                escape(u, true, out);
                out.push('"');
                scope.push((p.clone(), u.clone()));
            }
            for &a in &n.attrs {
                let an = t.node(a);
                let q = an.name.as_ref().unwrap();
                out.push(' ');
                out.push_str(&q.lexical());
                out.push_str("=\"");
                let uri =
                    q.uri.is_empty() && URI_ATTRS.iter().any(|u| q.local.eq_ignore_ascii_case(u));
                html_escape_attr(&an.value, uri, out);
                out.push('"');
            }
            out.push('>');
            let local = name.local.to_ascii_lowercase();
            if local == "head" {
                out.push_str(
                    "<META http-equiv=\"Content-Type\" content=\"text/html; charset=utf-16\">",
                );
            }
            if VOID.contains(&local.as_str()) {
                scope.truncate(depth);
                return;
            }
            let raw = local == "script" || local == "style";
            for &c in &n.children {
                html_node(t, c, scope, out, raw);
            }
            out.push_str("</");
            out.push_str(&name.lexical());
            out.push('>');
            scope.truncate(depth);
        }
        Kind::Document | Kind::Attribute => {}
    }
}

fn html_escape_text(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '\n' => out.push_str("\r\n"),
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
}

fn html_escape_attr(s: &str, uri: bool, out: &mut String) {
    let chars: Vec<char> = s.chars().collect();
    for (k, &c) in chars.iter().enumerate() {
        match c {
            // `&{` starts a script macro in HTML 4 and is kept.
            '&' if chars.get(k + 1) == Some(&'{') => out.push('&'),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            c if uri && !c.is_ascii() => {
                let mut b = [0u8; 4];
                for byte in c.encode_utf8(&mut b).bytes() {
                    out.push_str(&format!("%{byte:02X}"));
                }
            }
            c => out.push(c),
        }
    }
}
