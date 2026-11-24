//! The built-in engine behind `Host`: same results and output files as the
//! .NET host, using the engine's fixtures (expected output written by the
//! .NET host).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use xylem_xslt::Host;
use xylem_xslt::compare::compare;

fn cases() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../xylem-xslt-engine/tests/cases")
}

#[test]
fn builtin_host_runs_jobs_like_the_dotnet_host() {
    let host = Host::new("builtin");
    assert!(host.is_builtin());
    let out = std::env::temp_dir().join(format!("xylem-builtin-host-{}", std::process::id()));
    std::fs::create_dir_all(&out).unwrap();
    let params: BTreeMap<String, String> = [("mode", "final"), ("pageCount", "4")]
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect();
    for case in ["basics", "params", "imports"] {
        let job = (cases().join("input.xml"), out.join(format!("{case}.xml")));
        let run = host.run(
            &cases().join(case).join("style.xsl"),
            std::slice::from_ref(&job),
            &params,
            false,
        );
        assert!(run.error.is_none(), "{case}: {:?}", run.error);
        assert_eq!(run.xslt.as_deref(), Some("2.0"));
        assert!(run.jobs[0].ok, "{case}: {:?}", run.jobs[0].error);
        let expected = std::fs::read(cases().join(case).join("expected.xml")).unwrap();
        let got = std::fs::read(&job.1).unwrap();
        assert!(compare(&expected, &got).identical, "{case} differs");
        assert_eq!(expected, got, "{case}: not byte-identical");
    }
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn builtin_host_reports_compile_errors() {
    let dir = std::env::temp_dir().join(format!("xylem-builtin-bad-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let bad = dir.join("bad.xsl");
    std::fs::write(
        &bad,
        r#"<xsl:stylesheet version="2.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform"><xsl:template match="/"><xsl:value-of select="1 +"/></xsl:template></xsl:stylesheet>"#,
    )
    .unwrap();
    let host = Host::builtin();
    let compiled = host.compile(std::slice::from_ref(&bad));
    assert!(!compiled[0].ok);
    let run = host.run(
        &bad,
        &[(dir.join("in.xml"), dir.join("out.xml"))],
        &BTreeMap::new(),
        false,
    );
    assert!(run.error.is_some());
    assert!(!run.jobs[0].ok);
    let _ = std::fs::remove_dir_all(&dir);
}
