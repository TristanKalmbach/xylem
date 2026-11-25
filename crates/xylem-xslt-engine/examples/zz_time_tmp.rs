use std::collections::HashMap;
use std::time::Instant;
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let ss = xylem_xslt_engine::Stylesheet::compile(std::path::Path::new(&a[0])).unwrap();
    let input = std::fs::read(&a[1]).unwrap();
    let mut best = f64::MAX;
    for _ in 0..10 {
        let s = Instant::now();
        let _o = xylem_xslt_engine::transform_bytes_to(
            &ss,
            &input,
            &HashMap::new(),
            xylem_xslt_engine::ResultForm::Serialize,
        )
        .unwrap();
        best = best.min(s.elapsed().as_secs_f64() * 1000.0);
    }
    println!("best of 10: {best:.1} ms");
}
