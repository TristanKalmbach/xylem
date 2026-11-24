//! Compiles each stylesheet given and reports success or the first error:
//! `cargo run -p xylem-xslt-engine --example compile -- a.xsl b.xsl ...`
fn main() {
    for p in std::env::args().skip(1) {
        match xylem_xslt_engine::Stylesheet::compile(std::path::Path::new(&p)) {
            Ok(_) => println!("ok      {p}"),
            Err(e) => println!("error   {p}: {}", e.lines().next().unwrap_or("")),
        }
    }
}
