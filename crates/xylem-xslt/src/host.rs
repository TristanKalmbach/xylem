//! Running stylesheets through a host program: `tools/saxon-dotnet`'s
//! `SaxonDotnetHost.exe`, or anything that takes the same arguments
//! (`run --stylesheet <xsl> --job <in>=<out>... --param name=value...`) and
//! prints one JSON line per job and a summary line.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// A program that runs stylesheets.
#[derive(Debug, Clone)]
pub struct Host {
    pub program: PathBuf,
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
    error: Option<String>,
    profile: Option<PathBuf>,
}

impl Host {
    pub fn new(program: impl Into<PathBuf>) -> Host {
        Host {
            program: program.into(),
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
        if profile {
            cmd.arg("--profile");
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
            error: None,
        };
        for line in stdout.lines().filter(|l| l.trim_start().starts_with('{')) {
            let Ok(l) = serde_json::from_str::<Line>(line) else {
                continue;
            };
            if l.summary == Some(true) {
                run.compile_ms = l.compile_ms;
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
