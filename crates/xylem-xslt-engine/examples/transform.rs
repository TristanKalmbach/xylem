//! Runs a stylesheet on a file and prints the result:
//! `cargo run -p xylem-xslt-engine --example transform -- style.xsl input.xml [name=value ...]`
use std::collections::HashMap;
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: transform STYLESHEET INPUT [NAME=VALUE ...]");
        std::process::exit(2);
    }
    let params: HashMap<String, String> = args[2..]
        .iter()
        .filter_map(|p| {
            p.split_once('=')
                .map(|(k, v)| (k.to_string(), v.to_string()))
        })
        .collect();
    let result = xylem_xslt_engine::Stylesheet::compile(Path::new(&args[0]))
        .and_then(|ss| xylem_xslt_engine::transform_file(&ss, Path::new(&args[1]), &params));
    match result {
        Ok(out) => {
            for m in &out.messages {
                eprintln!("message: {m}");
            }
            println!("{}", out.text);
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}
