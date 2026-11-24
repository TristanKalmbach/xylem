//! Writing a result tree as text.
//!
//! [`host_style`] writes what the reference publishing host writes: the
//! `OuterXml` of a .NET `XmlDocument` loaded from the result, so empty
//! elements are `<a />`, `xsl:output` has no effect and there is no XML
//! declaration. Namespace declarations are written where a binding first
//! appears or changes.

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
            scope.extend(decls);
            for &a in &n.attrs {
                let an = t.node(a);
                out.push(' ');
                out.push_str(&an.name.as_ref().unwrap().lexical());
                out.push_str("=\"");
                escape(&an.value, true, out);
                out.push('"');
            }
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
