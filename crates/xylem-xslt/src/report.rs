//! A check's results as a standalone HTML page: every test with its outcome
//! and time, and for each test whose output changed, every place it changed
//! with the text around it, what was there and what is there now.

use crate::check::{CheckReport, Outcome};
use crate::compare::normalise;
use std::fmt::Write as _;
use std::path::Path;

/// Characters of unchanged text shown on each side of a difference.
const CONTEXT: usize = 160;

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

fn file_link(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    let url = if s.starts_with('/') {
        format!("file://{s}")
    } else {
        format!("file:///{s}")
    };
    format!(
        "<a href=\"{}\">{}</a>",
        esc(&url.replace(' ', "%20")),
        esc(&p.to_string_lossy())
    )
}

/// The unchanged text before and after the `len` bytes at `start` of the
/// baseline, up to [`CONTEXT`] characters each side.
fn context(base: &str, start: usize, len: usize) -> Option<(String, String)> {
    let end = start.checked_add(len)?;
    if end > base.len() || !base.is_char_boundary(start) || !base.is_char_boundary(end) {
        return None;
    }
    let before: Vec<char> = base[..start].chars().rev().take(CONTEXT).collect();
    let before: String = before.into_iter().rev().collect();
    let after: String = base[end..].chars().take(CONTEXT).collect();
    Some((before, after))
}

/// The report as an HTML page.
pub fn html(report: &CheckReport) -> String {
    let mut same = 0;
    let mut different = 0;
    let mut failed = 0;
    let mut unchecked = 0;
    for r in &report.results {
        match r.outcome {
            Outcome::Identical { .. } => same += 1,
            Outcome::Different(_) => different += 1,
            Outcome::Failed(_) => failed += 1,
            Outcome::NoBaseline => unchecked += 1,
        }
    }
    let mut out = String::new();
    out.push_str(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Stylesheet check</title>
<style>
:root { --bg:#fff; --fg:#1d2024; --muted:#5f6670; --line:#dde1e6; --code:#f5f6f8;
  --del:#fde2e1; --del-fg:#8f1d1a; --ins:#dcf3e3; --ins-fg:#14532d; --bad:#b42318; --good:#1a7f37; }
@media (prefers-color-scheme: dark) { :root { --bg:#16181b; --fg:#e6e8eb; --muted:#9aa1ab; --line:#2c3036;
  --code:#1f2226; --del:#4a1f1e; --del-fg:#ffb4ae; --ins:#173a24; --ins-fg:#a6e9bb; --bad:#ff8a80; --good:#7ee2a0; } }
body { margin:0; background:var(--bg); color:var(--fg); font:15px/1.5 system-ui, sans-serif; }
main { max-width:1100px; margin:0 auto; padding:24px 16px 64px; }
h1 { font-size:22px; margin:0 0 4px; } h2 { font-size:17px; margin:32px 0 8px; }
.muted { color:var(--muted); } a { color:inherit; }
table { border-collapse:collapse; width:100%; margin-top:16px; }
th, td { text-align:left; padding:6px 8px; border-bottom:1px solid var(--line); vertical-align:top; }
th { font-weight:600; color:var(--muted); font-size:13px; }
.bad { color:var(--bad); font-weight:600; } .good { color:var(--good); }
.diff { border:1px solid var(--line); border-radius:8px; margin:12px 0; overflow:hidden; }
.diff header { padding:8px 12px; background:var(--code); font-size:13px; }
.diff pre { margin:0; padding:10px 12px; white-space:pre-wrap; word-break:break-all;
  font:13px/1.5 ui-monospace, Consolas, monospace; }
del { background:var(--del); color:var(--del-fg); text-decoration:none; }
ins { background:var(--ins); color:var(--ins-fg); text-decoration:none; }
.label { display:inline-block; width:3.5em; color:var(--muted); }
</style></head><body><main>
"#,
    );
    let _ = write!(
        out,
        "<h1>Stylesheet check</h1><p class=\"muted\">{}<br>compared with {}{}</p>",
        esc(&report.stylesheet.to_string_lossy()),
        esc(&report.baseline.describe()),
        report
            .baseline_commit
            .as_deref()
            .map(|c| format!(" (commit {})", esc(&c[..c.len().min(12)])))
            .unwrap_or_default()
    );
    if let Some(e) = &report.error {
        let _ = write!(
            out,
            "<p class=\"bad\">The check could not run: {}</p>",
            esc(e)
        );
    }
    let mut summary = vec![format!("{same} the same")];
    if different > 0 {
        summary.push(format!("<span class=\"bad\">{different} different</span>"));
    }
    if failed > 0 {
        summary.push(format!("<span class=\"bad\">{failed} failed</span>"));
    }
    if unchecked > 0 {
        summary.push(format!("{unchecked} with nothing to compare with"));
    }
    if report.cancelled {
        summary.push("stopped before the end".into());
    }
    let _ = write!(out, "<p><strong>{}</strong></p>", summary.join(", "));

    out.push_str("<table><thead><tr><th>Test</th><th>Result</th><th>Time</th></tr></thead><tbody>");
    for (i, r) in report.results.iter().enumerate() {
        let result = match &r.outcome {
            Outcome::Identical { .. } => "<span class=\"good\">same</span>".to_string(),
            Outcome::Different(c) => format!(
                "<a class=\"bad\" href=\"#t{i}\">{} place{} different</a>",
                c.count,
                if c.count == 1 { "" } else { "s" }
            ),
            Outcome::Failed(e) => format!("<span class=\"bad\">failed</span> {}", esc(e)),
            Outcome::NoBaseline => "<span class=\"muted\">nothing to compare with</span>".into(),
        };
        let time = match (r.ms, r.baseline_ms) {
            (Some(a), Some(b)) => format!("{a} ms (was {b} ms)"),
            (Some(a), None) => format!("{a} ms"),
            _ => String::new(),
        };
        let _ = write!(
            out,
            "<tr><td>{}</td><td>{result}</td><td>{time}</td></tr>",
            esc(&r.name)
        );
    }
    out.push_str("</tbody></table>");

    for (i, r) in report.results.iter().enumerate() {
        let Outcome::Different(c) = &r.outcome else {
            continue;
        };
        let _ = write!(out, "<h2 id=\"t{i}\">{}</h2>", esc(&r.name));
        if let (Some(base), Some(new)) = (&r.baseline_output, &r.output) {
            let _ = write!(
                out,
                "<p class=\"muted\">Was: {}<br>Now: {}</p>",
                file_link(base),
                file_link(new)
            );
        }
        if c.coarse {
            out.push_str(
                "<p class=\"muted\">The outputs differ so much that the listing was cut short.</p>",
            );
        }
        let base_text = r
            .baseline_output
            .as_ref()
            .and_then(|p| std::fs::read(p).ok())
            .map(|b| normalise(&b));
        for d in &c.differences {
            let mut where_ = esc(&d.path);
            if let Some(id) = &d.near_id {
                let _ = write!(where_, " · near id <code>{}</code>", esc(id));
            }
            let _ = write!(out, "<div class=\"diff\"><header>{where_}</header><pre>");
            match base_text
                .as_deref()
                .and_then(|b| context(b, d.offset, d.baseline_len))
            {
                Some((before, after)) => {
                    let _ = write!(
                        out,
                        "…{}<del>{}</del><ins>{}</ins>{}…",
                        esc(&before),
                        esc(&d.baseline),
                        esc(&d.candidate),
                        esc(&after)
                    );
                }
                None => {
                    let _ = write!(
                        out,
                        "<span class=\"label\">was</span><del>{}</del>\n<span class=\"label\">now</span><ins>{}</ins>",
                        esc(&d.baseline),
                        esc(&d.candidate)
                    );
                }
            }
            out.push_str("</pre></div>");
        }
        if c.count > c.differences.len() {
            let _ = write!(
                out,
                "<p class=\"muted\">and {} more place{}</p>",
                c.count - c.differences.len(),
                if c.count - c.differences.len() == 1 {
                    ""
                } else {
                    "s"
                }
            );
        }
    }
    out.push_str("</main></body></html>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::{Baseline, TestResult};
    use crate::compare::compare;

    #[test]
    fn a_difference_is_shown_with_the_text_around_it() {
        let dir = std::env::temp_dir().join("xylem-xslt-report");
        std::fs::create_dir_all(&dir).unwrap();
        let base = dir.join("was.fo");
        let new = dir.join("now.fo");
        let a = "<root><block>Hello world, this is the <b>first</b> page.</block></root>";
        let b = "<root><block>Hello world, this is the <b>second</b> page.</block></root>";
        std::fs::write(&base, a).unwrap();
        std::fs::write(&new, b).unwrap();
        let c = compare(a.as_bytes(), b.as_bytes());
        let report = CheckReport {
            stylesheet: "book.xsl".into(),
            baseline: Baseline::Saved,
            baseline_commit: None,
            results: vec![TestResult {
                name: "Manual <1>".into(),
                outcome: Outcome::Different(c),
                ms: Some(12),
                baseline_ms: None,
                output: Some(new),
                baseline_output: Some(base),
            }],
            error: None,
            cancelled: false,
        };
        let page = html(&report);
        assert!(page.contains("Manual &lt;1&gt;"));
        assert!(page.contains("<del>first</del><ins>second</ins>"), "{page}");
        assert!(page.contains("this is the &lt;b&gt;"));
        assert!(page.contains("1 place different"));
    }
}
