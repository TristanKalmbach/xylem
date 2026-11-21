//! Prints the minimal valid markup for an element, as the editor would
//! generate it: `cargo run -p xylem-schema --example skeleton -- <schema.xsd|.dtd> <element>`.

fn print(s: &xylem_schema::Skeleton, depth: usize) {
    let pad = "  ".repeat(depth);
    let attrs: String = s
        .attrs
        .iter()
        .map(|(k, v)| format!(" {k}=\"{v}\""))
        .collect();
    if s.children.is_empty() {
        let body = if s.wants_text { "…" } else { "" };
        println!("{pad}<{}{attrs}>{body}</{}>", s.name, s.name);
        return;
    }
    println!("{pad}<{}{attrs}>", s.name);
    for c in &s.children {
        print(c, depth + 1);
    }
    println!("{pad}</{}>", s.name);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [path, element] = args.as_slice() else {
        eprintln!("usage: skeleton <schema.xsd|schema.dtd> <element>");
        std::process::exit(2);
    };
    let grammar = if path.ends_with(".dtd") {
        xylem_schema::Grammar::from_dtd(path)
    } else {
        xylem_schema::Grammar::from_xsd(path)
    }
    .unwrap_or_else(|e| panic!("{path}: {e}"));
    let decl = grammar
        .global_decl(element)
        .unwrap_or_else(|| panic!("no global element `{element}`"));
    print(&grammar.required_skeleton(decl), 0);
}
