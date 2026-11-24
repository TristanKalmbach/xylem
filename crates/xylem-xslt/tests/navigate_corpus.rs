//! Indexes the stylesheets that use `XYLEM_XSLT_NAV_FILE` (a shared module,
//! say) and prints how long that takes and what the index holds, to check
//! navigation on real stylesheets.
//!
//! ```text
//! XYLEM_XSLT_NAV_FILE=/path/to/common.xsl cargo test -p xylem-xslt --release \
//!     --test navigate_corpus -- --ignored --nocapture
//! ```

use std::path::PathBuf;
use std::time::Instant;
use xylem_xslt::navigate::{Index, tops_using};

#[test]
#[ignore = "needs XYLEM_XSLT_NAV_FILE"]
fn index_the_stylesheets_using_a_module() {
    let Some(file) = std::env::var_os("XYLEM_XSLT_NAV_FILE").map(PathBuf::from) else {
        return;
    };
    let started = Instant::now();
    let tops = tops_using(&file);
    let found = started.elapsed();
    let index = Index::build(&tops, &|_| None).expect("indexed");
    println!(
        "{} stylesheets use it (found in {:?}); {} modules indexed in {:?}",
        tops.len(),
        found,
        index.stylesheet.modules.len(),
        started.elapsed() - found
    );
    let text = std::fs::read_to_string(&file).unwrap();
    let mut shown = 0;
    for (at, _) in text
        .match_indices("<xsl:function ")
        .chain(text.match_indices("<xsl:template name="))
    {
        let Some(el) = index.element_at(&file, at) else {
            continue;
        };
        for r in index.references_at(&file, el) {
            println!(
                "{} {}: {} definition(s), {} usage(s)",
                r.kind.label(),
                r.name,
                r.definitions.len(),
                r.usages.len()
            );
        }
        shown += 1;
        if shown == 8 {
            break;
        }
    }
}
