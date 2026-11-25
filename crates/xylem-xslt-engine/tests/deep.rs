//! Deeply nested documents: building the tree, copying it and writing it
//! must not use the thread's stack in proportion to the depth.
//!
//! Each test runs on a thread with a 1 MiB stack, the size of a Windows
//! program's main thread, so a recursive walk would overflow it.

use std::collections::HashMap;
use std::path::PathBuf;
use xylem_xslt_engine::{Output, Stylesheet, transform_bytes};

const DEPTH: usize = 10_000;
const STACK: usize = 1 << 20;

/// Runs `f` on a thread with a [`STACK`]-sized stack.
fn on_small_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(STACK)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap()
}

/// `depth` nested `open` … `close` pairs around `inner`.
fn nested(open: &str, close: &str, inner: &str, depth: usize) -> String {
    let mut s = String::with_capacity((open.len() + close.len()) * depth + inner.len());
    for _ in 0..depth {
        s.push_str(open);
    }
    s.push_str(inner);
    for _ in 0..depth {
        s.push_str(close);
    }
    s
}

/// Compiles `xsl` (written to a temporary folder) and transforms `input`.
fn transform(name: &str, xsl: &str, input: &str) -> Output {
    let dir: PathBuf = std::env::temp_dir().join(format!(
        "xylem-xslt-engine-deep-{}-{name}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("t.xsl");
    std::fs::write(&path, xsl).unwrap();
    let ss = Stylesheet::compile(&path).unwrap();
    let out = transform_bytes(&ss, input.as_bytes(), &HashMap::new(), false).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    out
}

fn stylesheet(version: &str, body: &str) -> String {
    format!(
        "<xsl:stylesheet version=\"{version}\" xmlns:xsl=\"http://www.w3.org/1999/XSL/Transform\">{body}</xsl:stylesheet>"
    )
}

#[test]
fn deep_input_with_a_trivial_stylesheet() {
    let out = on_small_stack(|| {
        let input = nested("<a x=\"1\">", "</a>", "t", DEPTH);
        let xsl = stylesheet("1.0", "<xsl:template match=\"/\"><r/></xsl:template>");
        transform("trivial", &xsl, &input)
    });
    assert_eq!(out.text, "<r />");
}

#[test]
fn deep_input_copied_whole() {
    let (out, input) = on_small_stack(|| {
        let input = nested("<a x=\"1\">", "</a>", "t", DEPTH);
        let xsl = stylesheet(
            "2.0",
            "<xsl:template match=\"/\"><xsl:copy-of select=\".\"/></xsl:template>",
        );
        (transform("copy", &xsl, &input), input)
    });
    assert_eq!(out.text, input);
}

#[test]
fn deep_input_string_value_and_descendants() {
    let out = on_small_stack(|| {
        let input = nested("<a>", "</a>", "t", DEPTH);
        let xsl = stylesheet(
            "2.0",
            "<xsl:template match=\"/\"><r n=\"{count(//*)}\" s=\"{string(.)}\"/></xsl:template>",
        );
        transform("string", &xsl, &input)
    });
    assert_eq!(out.text, format!("<r n=\"{DEPTH}\" s=\"t\" />"));
}

#[test]
fn deep_input_with_namespaces_and_stripped_space() {
    let out = on_small_stack(|| {
        // Every level redeclares the prefix and holds whitespace that
        // `xsl:strip-space` drops, except under `xml:space="preserve"`.
        let mut input = String::from("<p:a xmlns:p=\"urn:p\">");
        input.push_str(&nested(
            "<p:a xmlns:p=\"urn:q\">\n  ",
            "\n</p:a>",
            "<p:b xml:space=\"preserve\"> </p:b>",
            DEPTH,
        ));
        input.push_str("</p:a>");
        let xsl = stylesheet(
            "2.0",
            "<xsl:strip-space elements=\"*\"/>\
             <xsl:template match=\"/\"><xsl:copy-of select=\".\"/></xsl:template>",
        );
        transform("namespaces", &xsl, &input)
    });
    let expected = format!(
        "<p:a xmlns:p=\"urn:p\"><p:a xmlns:p=\"urn:q\">{}</p:a></p:a>",
        nested(
            "<p:a>",
            "</p:a>",
            "<p:b xml:space=\"preserve\"> </p:b>",
            DEPTH - 1
        )
    );
    assert_eq!(out.text, expected);
}

#[test]
fn deep_result_written_as_html() {
    let out = on_small_stack(|| {
        let input = nested("<div>", "</div>", "t", DEPTH);
        let xsl = stylesheet(
            "1.0",
            "<xsl:output method=\"html\" indent=\"no\"/>\
             <xsl:template match=\"/\"><html><body><xsl:copy-of select=\".\"/></body></html></xsl:template>",
        );
        transform("html", &xsl, &input)
    });
    assert_eq!(
        out.serialized,
        format!(
            "<html><body>{}</body></html>",
            nested("<div>", "</div>", "t", DEPTH)
        )
    );
}
