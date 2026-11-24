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

#[test]
fn builtin_host_writes_profiles_like_the_dotnet_host() {
    let out = std::env::temp_dir().join(format!("xylem-builtin-profile-{}", std::process::id()));
    std::fs::create_dir_all(&out).unwrap();
    let job = (cases().join("input.xml"), out.join("params.xml"));
    let run = Host::builtin().run(
        &cases().join("params").join("style.xsl"),
        std::slice::from_ref(&job),
        &BTreeMap::new(),
        true,
    );
    assert!(run.jobs[0].ok, "{:?}", run.jobs[0].error);
    let path = run.jobs[0].profile.clone().expect("a profile");
    let p = xylem_xslt::profile::Profile::load(&path).unwrap();
    let chapter = p
        .templates
        .iter()
        .find(|t| t.pattern.as_deref() == Some("chapter"))
        .expect("the chapter rule");
    assert_eq!(
        (chapter.kind.as_str(), chapter.calls, chapter.line),
        ("template", 2, 36)
    );
    assert!(chapter.path().ends_with("style.xsl"));
    let label = p
        .templates
        .iter()
        .find(|t| t.name.as_deref() == Some("my:label"))
        .unwrap();
    assert_eq!((label.kind.as_str(), label.calls), ("function", 2));
    // Own time never exceeds total time.
    assert!(p.templates.iter().all(|t| t.self_ms <= t.total_ms + 1e-9));
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn builtin_host_serializes_like_the_dotnet_host() {
    let out = std::env::temp_dir().join(format!("xylem-builtin-html-{}", std::process::id()));
    std::fs::create_dir_all(&out).unwrap();
    let mut host = Host::builtin();
    host.serialize = true;
    let job = (cases().join("input.xml"), out.join("html1.html"));
    let run = host.run(
        &cases().join("html1").join("style.xsl"),
        std::slice::from_ref(&job),
        &BTreeMap::new(),
        false,
    );
    assert!(run.jobs[0].ok, "{:?}", run.jobs[0].error);
    assert_eq!(run.xslt.as_deref(), Some("1.0"));
    let expected = std::fs::read(cases().join("html1").join("expected.xml")).unwrap();
    assert_eq!(std::fs::read(&job.1).unwrap(), expected);
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn compare_engines_reports_agreement_and_failures() {
    use std::sync::atomic::AtomicBool;
    use xylem_xslt::{Baseline, CheckEnv, EngineComparison, Suite, check, compare_engines};
    let dir = std::env::temp_dir().join(format!("xylem-engines-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let suite_file = dir.join("suite.toml");
    let input = cases().join("input.xml");
    let expected = cases().join("basics").join("expected.xml");
    std::fs::write(
        &suite_file,
        format!(
            "[params]\nmode = \"final\"\npageCount = 4\n[[test]]\nname = \"basics\"\ninput = '{}'\nexpected = '{}'\n",
            input.display(),
            expected.display()
        ),
    )
    .unwrap();
    let suite = Suite::load(&suite_file).unwrap();
    let style = cases().join("basics").join("style.xsl");
    let cancel = AtomicBool::new(false);
    let host = Host::builtin();
    let mut progress = |_: String| {};
    let run = |reference: &Host| {
        let work = dir.join("work");
        let mut progress = |_: String| {};
        let mut report = check(
            &suite,
            &style,
            &Baseline::Saved,
            CheckEnv {
                host: &host,
                work: &work,
                progress: &mut progress,
                cancel: &cancel,
            },
        );
        compare_engines(
            &mut report,
            &suite,
            reference,
            &work,
            &mut progress,
            &cancel,
        );
        report
    };
    let same = run(&Host::builtin());
    assert_eq!(same.results[0].engines, Some(EngineComparison::Same));
    assert!(same.engines_agree() && same.all_identical());
    let missing = run(&Host::new(dir.join("no-such-engine.exe")));
    assert!(matches!(
        missing.results[0].engines,
        Some(EngineComparison::Failed(_))
    ));
    assert!(!missing.engines_agree());
    let _ = &mut progress;
    let _ = std::fs::remove_dir_all(&dir);
}
