//! Writes a new data module skeleton: `new_dm <issue> <kind> <DMC-...> <out-dir>`.
use xylem_s1000d::*;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let issue = Issue::parse(&a[1]).expect("issue");
    let kind = SchemaKind::from_stem(&a[2]).expect("kind");
    let dmc = Dmc::parse(&a[3]).expect("dmc");
    let xml = new_data_module(issue, kind, &dmc, "Hydraulic pump", Some("Description"), ("en", "US"), IssueDate::new(2026, 11, 20)).expect("template");
    let name = csdb_filename(&dmc.to_string(), &IssueInfo::new("001", "00"), &Language::new("en", "US"));
    std::fs::write(std::path::Path::new(&a[4]).join(&name), xml).unwrap();
    println!("{name}");
}
