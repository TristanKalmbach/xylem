//! `xylem-xslt`: a command-line XSLT 2.0 processor built on Xylem's engine.
//!
//! ```text
//! xylem-xslt transform --stylesheet <file.xsl> --input <file.xml> [--output <file>]
//!                      [--param name=value ...] [--serialize] [--no-file-access]
//! xylem-xslt compile   --stylesheet <file.xsl> ...
//! xylem-xslt serve
//! ```
//!
//! `transform` writes the result the way a publishing service saves it
//! (the `XmlDocument.OuterXml` form, `xsl:output` ignored; a result that is
//! not one well-formed document is an error, as it is there) unless
//! `--serialize` asks for the stylesheet's `xsl:output`. Without
//! `--output` the result goes to standard output. Serialized text is
//! written as UTF-8 and labelled so (the service's string names `utf-16`);
//! `serve` returns it as the service has it.
//!
//! `serve` keeps compiled stylesheets in memory and reads one JSON request
//! per line on standard input, answering each with one JSON line on
//! standard output, so another program can run many transformations
//! without starting a process for each:
//!
//! ```text
//! → {"id":1,"stylesheet":"C:/s/book.xsl","input":"C:/in/a.xml","output":"C:/out/a.fo","params":{"mode":"final"}}
//! ← {"id":1,"ok":true,"ms":42}
//! → {"id":2,"stylesheet":"C:/s/book.xsl","input_text":"<book/>","serialize":false}
//! ← {"id":2,"ok":true,"ms":3,"output_text":"<fo:root …/>"}
//! → {"id":3,"stylesheet":"C:/s/missing.xsl","input_text":"<a/>"}
//! ← {"id":3,"ok":false,"error":"…","at":{"message":"…","file":"…","line":12,"column":null,"stack":[…]}}
//! ```
//!
//! A request names `input` (a file) or `input_text`; the result goes to
//! `output` (a file) or is returned as `output_text`. `params`,
//! `serialize`, `no_file_access` and `versions` (`reference-host`, the
//! default: only a root `version="2.0"` runs as XSLT 2.0, as the reference
//! publishing service decides; or `2-and-above`) are optional. A stylesheet is compiled
//! again when its file changes; `{"command":"clear"}` drops every compiled
//! stylesheet and `{"command":"quit"}` ends the session.

/// The engine allocates many small values; mimalloc makes transformations
/// about a third faster than the system allocator.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use serde::Deserialize;
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Instant, SystemTime};
use xylem_xslt_engine::{
    ResultForm, Stylesheet, VersionRule, XsltError, transform_bytes_to, with_file_access,
};

const USAGE: &str = "usage: xylem-xslt transform --stylesheet <file.xsl> --input <file.xml> [--output <file>] [--param name=value ...] [--serialize] [--no-file-access] [--versions reference-host|2-and-above]
       xylem-xslt compile --stylesheet <file.xsl> ...
       xylem-xslt serve";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let r = match args.first().map(String::as_str) {
        Some("transform") => transform(&args[1..]),
        Some("compile") => compile(&args[1..]),
        Some("serve") => serve(),
        Some("--version") => {
            println!("xylem-xslt {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => Err(USAGE.to_string()),
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn transform(args: &[String]) -> Result<(), String> {
    let mut stylesheet = None;
    let mut input = None;
    let mut output = None;
    let mut params = HashMap::new();
    let mut serialize = false;
    let mut files = true;
    let mut versions = VersionRule::ReferenceHost;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = || {
            it.next()
                .cloned()
                .ok_or(format!("{a} needs a value\n{USAGE}"))
        };
        match a.as_str() {
            "--stylesheet" => stylesheet = Some(PathBuf::from(value()?)),
            "--input" => input = Some(PathBuf::from(value()?)),
            "--output" => output = Some(PathBuf::from(value()?)),
            "--param" => {
                let p = value()?;
                let (k, v) = p
                    .split_once('=')
                    .ok_or(format!("--param takes name=value, got {p}"))?;
                params.insert(k.to_string(), v.to_string());
            }
            "--serialize" => serialize = true,
            "--no-file-access" => files = false,
            "--versions" => versions = version_rule(&value()?)?,
            _ => return Err(format!("unknown argument {a}\n{USAGE}")),
        }
    }
    let stylesheet = stylesheet.ok_or(USAGE)?;
    let input = input.ok_or(USAGE)?;
    let mut ss = Stylesheet::compile(&stylesheet)?;
    ss.version_rule = versions;
    let bytes = std::fs::read(&input).map_err(|e| format!("{}: {e}", input.display()))?;
    let write = if serialize {
        ResultForm::Serialize
    } else {
        ResultForm::Publish
    };
    let mut out = with_file_access(files, || transform_bytes_to(&ss, &bytes, &params, write))?;
    let text = if serialize {
        // A file or terminal, not the service's string: label it as the
        // UTF-8 it is.
        xylem_xslt_engine::label_utf8(&out.serialized).into_owned()
    } else {
        out.published()?;
        std::mem::take(&mut out.text)
    };
    for m in &out.messages {
        eprintln!("message: {m}");
    }
    for w in &out.warnings {
        eprintln!("warning: {w}");
    }
    match output {
        Some(p) => {
            std::fs::write(&p, text).map_err(|e| format!("{}: {e}", p.display()))?;
            out.write_secondary(&p, files)
        }
        None if !out.results.is_empty() => {
            Err("the stylesheet writes secondary results; give --output".into())
        }
        None => {
            print!("{text}");
            Ok(())
        }
    }
}

/// `--versions` / `"versions"`: which stylesheets run as XSLT 2.0.
fn version_rule(s: &str) -> Result<VersionRule, String> {
    match s {
        "reference-host" => Ok(VersionRule::ReferenceHost),
        "2-and-above" => Ok(VersionRule::TwoAndAbove),
        _ => Err(format!(
            "--versions takes reference-host or 2-and-above, got {s}"
        )),
    }
}

fn compile(args: &[String]) -> Result<(), String> {
    let mut failed = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a != "--stylesheet" {
            return Err(format!("unknown argument {a}\n{USAGE}"));
        }
        let p = it.next().ok_or(USAGE)?;
        match Stylesheet::compile(Path::new(p)) {
            Ok(_) => println!("ok      {p}"),
            Err(e) => {
                failed = true;
                let e = XsltError::from_message(&e);
                match (&e.file, e.line) {
                    (Some(f), Some(l)) => println!("error   {f}:{l}: {}", e.message),
                    _ => println!("error   {p}: {}", e.message),
                }
            }
        }
    }
    if failed {
        Err("some stylesheets did not compile".into())
    } else {
        Ok(())
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Request {
    id: serde_json::Value,
    command: Option<String>,
    stylesheet: Option<PathBuf>,
    input: Option<PathBuf>,
    input_text: Option<String>,
    output: Option<PathBuf>,
    params: HashMap<String, serde_json::Value>,
    serialize: bool,
    no_file_access: bool,
    /// `reference-host` (the default) or `2-and-above`.
    versions: Option<String>,
}

struct Cached {
    modified: Option<SystemTime>,
    ss: Stylesheet,
}

fn serve() -> Result<(), String> {
    let mut cache: HashMap<PathBuf, Cached> = HashMap::new();
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Request>(&line) {
            Err(e) => serde_json::json!({"ok": false, "error": format!("bad request: {e}")}),
            Ok(req) => match req.command.as_deref() {
                Some("quit") => break,
                Some("clear") => {
                    cache.clear();
                    serde_json::json!({"id": req.id, "ok": true})
                }
                Some(other) => {
                    serde_json::json!({"id": req.id, "ok": false, "error": format!("unknown command {other}")})
                }
                None => handle(&req, &mut cache),
            },
        };
        writeln!(stdout, "{reply}").map_err(|e| e.to_string())?;
        stdout.flush().map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn handle(req: &Request, cache: &mut HashMap<PathBuf, Cached>) -> serde_json::Value {
    let started = Instant::now();
    // Errors carry their location too, so an editor can jump to it.
    let fail = |e: String| {
        let at: serde_json::Value =
            serde_json::from_str(&XsltError::from_message(&e).to_json()).unwrap_or_default();
        serde_json::json!({"id": req.id, "ok": false, "error": e, "at": at})
    };
    let Some(path) = &req.stylesheet else {
        return fail("no stylesheet".into());
    };
    // Compiled once, again when the file changes (its includes are not
    // watched; send {"command":"clear"} after changing them).
    let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    let stale = cache.get(path).is_none_or(|c| c.modified != modified);
    if stale {
        match Stylesheet::compile(path) {
            Ok(ss) => {
                cache.insert(path.clone(), Cached { modified, ss });
            }
            Err(e) => return fail(e),
        }
    }
    let rule = match req.versions.as_deref().map(version_rule).transpose() {
        Ok(r) => r.unwrap_or_default(),
        Err(e) => return fail(e),
    };
    let ss = &mut cache.get_mut(path).expect("compiled above").ss;
    ss.version_rule = rule;
    let ss = &*ss;
    let bytes = match (&req.input, &req.input_text) {
        (Some(p), _) => match std::fs::read(p) {
            Ok(b) => b,
            Err(e) => return fail(format!("{}: {e}", p.display())),
        },
        (None, Some(t)) => t.clone().into_bytes(),
        (None, None) => return fail("no input or input_text".into()),
    };
    let params: HashMap<String, String> = req
        .params
        .iter()
        .map(|(k, v)| {
            let v = match v {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            (k.clone(), v)
        })
        .collect();
    let result = with_file_access(!req.no_file_access, || {
        let write = if req.serialize {
            ResultForm::Serialize
        } else {
            ResultForm::Publish
        };
        transform_bytes_to(ss, &bytes, &params, write)
    });
    let mut out = match result {
        Ok(o) => o,
        Err(e) => return fail(e),
    };
    let text = if req.serialize {
        std::mem::take(&mut out.serialized)
    } else {
        if let Err(e) = out.published() {
            return fail(e);
        }
        std::mem::take(&mut out.text)
    };
    let ms = started.elapsed().as_millis() as u64;
    match &req.output {
        Some(p) => match std::fs::write(p, text)
            .map_err(|e| format!("{}: {e}", p.display()))
            .and_then(|()| out.write_secondary(p, !req.no_file_access))
        {
            Ok(()) => {
                serde_json::json!({"id": req.id, "ok": true, "ms": ms, "messages": out.messages, "warnings": out.warnings})
            }
            Err(e) => fail(e),
        },
        None if !out.results.is_empty() => {
            fail("the stylesheet writes secondary results; give an output path".into())
        }
        None => {
            serde_json::json!({"id": req.id, "ok": true, "ms": ms, "messages": out.messages, "warnings": out.warnings, "output_text": text})
        }
    }
}
