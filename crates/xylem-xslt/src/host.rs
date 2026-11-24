//! Running stylesheets through a host program: `tools/saxon-dotnet`'s
//! `SaxonDotnetHost.exe`, or anything that takes the same arguments
//! (`run --stylesheet <xsl> --job <in>=<out>... --param name=value...`) and
//! prints one JSON line per job and a summary line.
//!
//! The program name [`BUILTIN`] selects Xylem's own engine
//! (`xylem-xslt-engine`) instead, run in this process with the same
//! results and output files.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// A program that runs stylesheets.
#[derive(Debug, Clone)]
pub struct Host {
    pub program: PathBuf,
    /// Runs per input; the median time is reported (timing runs only).
    pub repeat: u32,
    /// Read the input as it is and write the result as the stylesheet's
    /// `xsl:output` asks, as a second step on text a publishing service
    /// already has (XSL-FO to HTML). Off: the result is written the way the
    /// service saves a publishing stylesheet's output.
    pub serialize: bool,
    /// Let XSLT 1.0 stylesheets run embedded scripts (`msxsl:script`).
    pub allow_scripts: bool,
}

/// Whether one stylesheet compiled.
#[derive(Debug, Clone)]
pub struct Compiled {
    pub stylesheet: PathBuf,
    pub ok: bool,
    pub error: Option<String>,
}

/// What became of one input.
#[derive(Debug, Clone)]
pub struct JobResult {
    pub input: PathBuf,
    pub output: PathBuf,
    pub ok: bool,
    /// Time the transform took, in milliseconds.
    pub ms: u64,
    pub error: Option<String>,
    /// Timings written by a run with profiling on.
    pub profile: Option<PathBuf>,
}

/// One run of the host: a stylesheet compiled once, applied to inputs.
#[derive(Debug, Clone)]
pub struct Run {
    pub jobs: Vec<JobResult>,
    pub compile_ms: Option<u64>,
    /// The XSLT version the host ran the stylesheet as ("1.0" or "2.0").
    pub xslt: Option<String>,
    /// Set when the stylesheet itself could not be compiled or the host
    /// could not run.
    pub error: Option<String>,
}

#[derive(Deserialize)]
struct Line {
    job: Option<usize>,
    summary: Option<bool>,
    ok: Option<bool>,
    ms: Option<u64>,
    compile_ms: Option<u64>,
    compiled: Option<bool>,
    xslt: Option<String>,
    error: Option<String>,
    profile: Option<PathBuf>,
}

/// The program name that selects the built-in engine.
pub const BUILTIN: &str = "builtin";

impl Host {
    /// The built-in engine (see [`BUILTIN`]).
    pub fn builtin() -> Host {
        Host::new(BUILTIN)
    }

    /// Whether this host is the built-in engine rather than a program.
    pub fn is_builtin(&self) -> bool {
        self.program.as_os_str().eq_ignore_ascii_case(BUILTIN)
    }

    pub fn new(program: impl Into<PathBuf>) -> Host {
        Host {
            program: program.into(),
            repeat: 1,
            serialize: false,
            allow_scripts: false,
        }
    }

    /// Runs `stylesheet` on each `(input, output)` pair with `params`;
    /// with `profile`, also times every template and function.
    pub fn run(
        &self,
        stylesheet: &Path,
        jobs: &[(PathBuf, PathBuf)],
        params: &BTreeMap<String, String>,
        profile: bool,
    ) -> Run {
        if self.is_builtin() {
            return run_builtin(self, stylesheet, jobs, params, profile);
        }
        let mut cmd = Command::new(&self.program);
        cmd.arg("run").arg("--stylesheet").arg(stylesheet);
        for (input, output) in jobs {
            let mut pair = input.as_os_str().to_owned();
            pair.push("=");
            pair.push(output.as_os_str());
            cmd.arg("--job").arg(pair);
        }
        for (k, v) in params {
            cmd.arg("--param").arg(format!("{k}={v}"));
        }
        if self.serialize {
            cmd.arg("--serialize");
        }
        if self.allow_scripts {
            cmd.arg("--allow-scripts");
        }
        if profile {
            cmd.arg("--profile");
        } else if self.repeat > 1 {
            cmd.arg("--repeat").arg(self.repeat.to_string());
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        quiet(&mut cmd);
        let failed = |error: String| Run {
            jobs: jobs
                .iter()
                .map(|(i, o)| JobResult {
                    input: i.clone(),
                    output: o.clone(),
                    ok: false,
                    ms: 0,
                    error: Some(error.clone()),
                    profile: None,
                })
                .collect(),
            compile_ms: None,
            xslt: None,
            error: Some(error),
        };
        let out = match cmd.output() {
            Ok(o) => o,
            Err(e) => {
                return failed(format!("Could not start {}: {e}", self.program.display()));
            }
        };
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        let mut run = Run {
            jobs: Vec::new(),
            compile_ms: None,
            xslt: None,
            error: None,
        };
        for line in stdout.lines().filter(|l| l.trim_start().starts_with('{')) {
            let Ok(l) = serde_json::from_str::<Line>(line) else {
                continue;
            };
            if l.summary == Some(true) {
                run.compile_ms = l.compile_ms;
                run.xslt = l.xslt;
                if l.compiled == Some(false) {
                    run.error = Some(
                        l.error
                            .unwrap_or_else(|| "The stylesheet did not compile.".into()),
                    );
                }
            } else if let Some(n) = l.job
                && let Some((input, output)) = jobs.get(n.wrapping_sub(1))
            {
                run.jobs.push(JobResult {
                    input: input.clone(),
                    output: output.clone(),
                    ok: l.ok == Some(true),
                    ms: l.ms.unwrap_or(0),
                    error: l.error,
                    profile: l.profile,
                });
            }
        }
        if let Some(e) = &run.error {
            return failed(e.clone());
        }
        if run.jobs.len() != jobs.len() {
            let detail = stderr.trim();
            return failed(if detail.is_empty() {
                format!(
                    "{} stopped without reporting every input.",
                    self.program.display()
                )
            } else {
                detail.to_string()
            });
        }
        run
    }
}

impl Host {
    /// Compiles each stylesheet as a run would, without running it.
    pub fn compile(&self, stylesheets: &[PathBuf]) -> Vec<Compiled> {
        #[derive(Deserialize)]
        struct CompileLine {
            ok: Option<bool>,
            error: Option<String>,
        }
        if self.is_builtin() {
            return stylesheets
                .iter()
                .map(|s| {
                    let r = xylem_xslt_engine::Stylesheet::compile(s);
                    Compiled {
                        stylesheet: s.clone(),
                        ok: r.is_ok(),
                        error: r.err(),
                    }
                })
                .collect();
        }
        let mut results = Vec::new();
        // In batches, to keep command lines short.
        for chunk in stylesheets.chunks(20) {
            let mut cmd = Command::new(&self.program);
            cmd.arg("compile");
            if self.allow_scripts {
                cmd.arg("--allow-scripts");
            }
            for s in chunk {
                cmd.arg("--stylesheet").arg(s);
            }
            cmd.stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            quiet(&mut cmd);
            let out = match cmd.output() {
                Ok(o) => o,
                Err(e) => {
                    let error = format!("Could not start {}: {e}", self.program.display());
                    results.extend(chunk.iter().map(|s| Compiled {
                        stylesheet: s.clone(),
                        ok: false,
                        error: Some(error.clone()),
                    }));
                    continue;
                }
            };
            let lines: Vec<CompileLine> = String::from_utf8_lossy(&out.stdout)
                .lines()
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect();
            for (i, s) in chunk.iter().enumerate() {
                let l = lines.get(i);
                results.push(Compiled {
                    stylesheet: s.clone(),
                    ok: l.and_then(|l| l.ok) == Some(true),
                    error: match l {
                        Some(l) => l.error.clone(),
                        None => Some("The engine did not report on this stylesheet.".into()),
                    },
                });
            }
        }
        results
    }
}

/// Runs the host below normal priority and without a console window, so a
/// long check doesn't get in the way of other work.
fn quiet(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;
        cmd.creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

/// Runs jobs on the built-in engine: compiled once, each output written
/// the way the .NET host writes it (UTF-8 with a byte order mark).
fn run_builtin(
    host: &Host,
    stylesheet: &Path,
    jobs: &[(PathBuf, PathBuf)],
    params: &BTreeMap<String, String>,
    profile: bool,
) -> Run {
    let failed = |error: String| Run {
        jobs: jobs
            .iter()
            .map(|(i, o)| JobResult {
                input: i.clone(),
                output: o.clone(),
                ok: false,
                ms: 0,
                error: Some(error.clone()),
                profile: None,
            })
            .collect(),
        compile_ms: None,
        xslt: None,
        error: Some(error),
    };
    if host.serialize {
        return failed("The built-in engine cannot write xsl:output results yet; use the .NET host for this step.".into());
    }
    if profile {
        return failed("The built-in engine cannot write timing profiles yet; use the .NET host to time templates.".into());
    }
    let started = std::time::Instant::now();
    let ss = match xylem_xslt_engine::Stylesheet::compile(stylesheet) {
        Ok(s) => s,
        Err(e) => return failed(e),
    };
    let compile_ms = started.elapsed().as_millis() as u64;
    let params: std::collections::HashMap<String, String> =
        params.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    let repeat = host.repeat.max(1);
    let mut out = Vec::new();
    for (input, output) in jobs {
        let mut times = Vec::new();
        let mut result = Err(String::new());
        for _ in 0..repeat {
            let t = std::time::Instant::now();
            result = xylem_xslt_engine::transform_file(&ss, input, &params);
            times.push(t.elapsed().as_millis() as u64);
            if result.is_err() {
                break;
            }
        }
        times.sort_unstable();
        let ms = times[times.len() / 2];
        let (ok, error) = match result {
            Ok(o) => match std::fs::write(output, o.host_bytes()) {
                Ok(()) => (true, None),
                Err(e) => (false, Some(format!("{}: {e}", output.display()))),
            },
            Err(e) => (false, Some(e)),
        };
        out.push(JobResult {
            input: input.clone(),
            output: output.clone(),
            ok,
            ms,
            error,
            profile: None,
        });
    }
    Run {
        jobs: out,
        compile_ms: Some(compile_ms),
        xslt: Some(if ss.version.starts_with('1') { "1.0" } else { "2.0" }.into()),
        error: None,
    }
}
