//! Round-trips every XML-ish file under a directory and reports mismatches.
use std::path::Path;
fn walk(p: &Path, out: &mut Vec<std::path::PathBuf>) {
    if let Ok(rd) = std::fs::read_dir(p) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if !p.ends_with(".git") && !p.ends_with("worktrees") {
                    walk(&p, out)
                }
            } else if matches!(
                p.extension()
                    .and_then(|e| e.to_str())
                    .map(|s| s.to_ascii_lowercase())
                    .as_deref(),
                Some("xml" | "xsd" | "ctm" | "xdp" | "tbr" | "xsl" | "xslt")
            ) {
                out.push(p)
            }
        }
    }
}
fn main() {
    let dir = std::env::args().nth(1).expect("dir");
    let mut files = Vec::new();
    walk(Path::new(&dir), &mut files);
    let (mut ok, mut bad, mut err) = (0, 0, 0);
    let t = std::time::Instant::now();
    for f in &files {
        let bytes = std::fs::read(f).unwrap();
        match xylem_dom::Document::from_bytes(&bytes) {
            Ok(d) => {
                if d.to_bytes() == bytes {
                    ok += 1
                } else {
                    bad += 1;
                    if bad < 10 {
                        println!("MISMATCH {}", f.display())
                    }
                }
            }
            Err(e) => {
                err += 1;
                if err < 15 {
                    println!("ERR {} {e}", f.display())
                }
            }
        }
    }
    println!(
        "files={} exact={ok} mismatch={bad} parse_err={err} in {:?}",
        files.len(),
        t.elapsed()
    );
}
