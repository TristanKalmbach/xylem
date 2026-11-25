//! Right-to-left paragraphs laid out through egui with the fonts the app
//! loads: carets, hit testing and selection stay consistent with the
//! paragraph's characters.

use super::*;
use crate::view::style::DefaultStyles;

fn layout_of(xml: &str, view: ViewMode, width: f32) -> DocLayout {
    let ctx = egui::Context::default();
    let fonts = crate::fonts::install(&ctx);
    crate::fonts::ensure(&ctx, &crate::fonts::scripts_in(xml, false));
    let doc = Document::from_str(xml).unwrap();
    let styles = DefaultStyles::compute(&doc);
    let mut cache = LayoutCache::default();
    let colors = HashMap::new();
    let collapsed = HashSet::new();
    let no_image = |_: NodeId| None;
    let mut out = None;
    // Fonts set on the context take effect at the start of the next pass.
    for _ in 0..2 {
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            let params = LayoutParams {
                view,
                width,
                zoom: 1.0,
                fonts: &fonts,
                tag_colors: &colors,
                dark: false,
                image_size: &no_image,
                collapsed: &collapsed,
            };
            out = Some(Layouter::run(
                ctx,
                &doc,
                &styles,
                &xylem_edit::NoRules,
                params,
                &mut cache,
            ));
        });
    }
    out.unwrap()
}

/// The paragraphs that went through the right-to-left path.
fn rtl_paras(l: &DocLayout) -> Vec<usize> {
    (0..l.paras.len())
        .filter(|&i| l.paras[i].bidi.is_some())
        .collect()
}

/// Every caret position between two characters that read the same way (and
/// at both ends) is found again by clicking on it. At a boundary between a
/// left-to-right and a right-to-left run one place on screen stands for two
/// positions, so those are not checked.
fn assert_caret_round_trip(l: &DocLayout, pi: usize) {
    let p = &l.paras[pi];
    let map = p.bidi.as_ref().unwrap();
    let n = map.len();
    for k in 0..=n {
        let inside_ligature = k > 0 && k < n && map.glyph[k - 1] == map.glyph[k];
        let mixed = k > 0 && k < n && map.rtl[k - 1] != map.rtl[k];
        let at_break = (k > 0 && map.breaks[k - 1]) || (k < n && map.breaks[k]);
        // Inside a tag chip or atom is not a caret position (the editor
        // snaps it to the chip's edges).
        let inside_object = p.segs.iter().any(|s| {
            !matches!(s.kind, SegKind::Text { .. } | SegKind::Generated(_))
                && s.chars.start < k
                && k < s.chars.end
        });
        if inside_ligature || mixed || at_break || inside_object {
            continue;
        }
        let r = l.caret_rect(Loc { para: pi, ch: k });
        let hit = l.hit(r.center()).unwrap();
        assert_eq!(
            (hit.para, hit.ch),
            (pi, k),
            "caret {k} of {n} drawn at {r:?} is hit as {}",
            hit.ch
        );
    }
}

fn assert_selection_covers_each_glyph_once(l: &DocLayout, pi: usize) {
    let p = &l.paras[pi];
    let map = p.bidi.as_ref().unwrap();
    let n = map.len();
    let rects = l.range_rects(pi, 0, n);
    let covered: f32 = rects.iter().map(|r| r.width()).sum();
    let mut seen = HashSet::new();
    let mut glyphs = 0.0;
    for i in 0..n {
        let v = map.glyph[i] as usize;
        if map.breaks[i] || !seen.insert(v) {
            continue;
        }
        let (x0, x1, _, _) = p.glyph_box(map, v);
        glyphs += x1 - x0;
    }
    assert!(
        (covered - glyphs).abs() < 0.5,
        "selection covers {covered}, glyphs are {glyphs} wide"
    );
    // No two rectangles on a row overlap.
    for (i, a) in rects.iter().enumerate() {
        for b in &rects[i + 1..] {
            let overlap = a.intersect(*b);
            assert!(
                overlap.width() <= 0.5 || overlap.height() <= 0.5,
                "{a:?} overlaps {b:?}"
            );
        }
    }
}

fn galley_text(l: &DocLayout, pi: usize) -> String {
    l.paras[pi].galley.job.text.clone()
}

#[test]
fn left_to_right_paragraphs_are_untouched() {
    let l = layout_of(
        "<doc><p>Plain text, café.</p></doc>",
        ViewMode::Normal,
        600.0,
    );
    assert!(rtl_paras(&l).is_empty());
}

#[test]
fn arabic_is_shaped_and_starts_at_the_right() {
    let l = layout_of("<doc><p>مرحبا بالعالم</p></doc>", ViewMode::Normal, 600.0);
    let paras = rtl_paras(&l);
    assert_eq!(paras.len(), 1);
    let pi = paras[0];
    let text = galley_text(&l, pi);
    // Every letter is in its presentation form; no plain Arabic letters are
    // left for the font to draw unjoined.
    assert!(
        text.chars().all(|c| c == ' ' || (c as u32) >= 0xFB50),
        "{text:?}"
    );
    let p = &l.paras[pi];
    assert!(p.bidi.as_ref().unwrap().base_rtl);
    // Right aligned: the line hangs from its anchor at the column's right
    // edge, well over to the right of a 600 px column.
    assert!(
        (p.rect.max.x - p.anchor.x).abs() < 1.0,
        "{:?} {:?}",
        p.rect,
        p.anchor
    );
    assert!(p.rect.min.x > 300.0, "line starts at {}", p.rect.min.x);
    assert_caret_round_trip(&l, pi);
    assert_selection_covers_each_glyph_once(&l, pi);
}

#[test]
fn mixed_hebrew_latin_and_numbers() {
    let l = layout_of(
        "<doc><p>שלום (world) 123 עולם</p><p>Hello שלום world</p></doc>",
        ViewMode::Normal,
        600.0,
    );
    let paras = rtl_paras(&l);
    assert_eq!(paras.len(), 2);
    // The first reads right to left; brackets are mirrored only inside
    // right-to-left runs, so "(world)" keeps its own.
    let first = galley_text(&l, paras[0]);
    assert_eq!(first, "םלוע 123 (world) םולש");
    assert!(l.paras[paras[0]].bidi.as_ref().unwrap().base_rtl);
    // The second reads left to right with the Hebrew word turned in place.
    let second = galley_text(&l, paras[1]);
    assert_eq!(second, "Hello םולש world");
    assert!(!l.paras[paras[1]].bidi.as_ref().unwrap().base_rtl);
    for pi in paras {
        assert_caret_round_trip(&l, pi);
        assert_selection_covers_each_glyph_once(&l, pi);
    }
}

#[test]
fn wrapped_paragraph_keeps_lines_in_reading_order() {
    let words = "אחת שתיים שלוש ארבע חמש שש שבע שמונה תשע עשר";
    let l = layout_of(
        &format!("<doc><p>{words} {words}</p></doc>"),
        ViewMode::Normal,
        160.0,
    );
    let pi = rtl_paras(&l)[0];
    let p = &l.paras[pi];
    let map = p.bidi.as_ref().unwrap();
    assert!(p.galley.rows.len() > 2, "{} rows", p.galley.rows.len());
    // Row by row, the characters follow on from each other: the first row
    // holds the first words.
    let mut next = 0;
    for r in &map.rows {
        assert_eq!(r.start, next);
        next = r.end;
    }
    assert_eq!(next as usize, map.len());
    // The first word is at the right end of the first row.
    let first_row: String = galley_text(&l, pi).lines().next().unwrap().to_string();
    assert!(first_row.trim_end().ends_with("תחא"), "{first_row:?}");
    // Home and End find each row's ends.
    for (ri, r) in map.rows.iter().enumerate() {
        let mid = Loc {
            para: pi,
            ch: (r.start as usize + r.end as usize) / 2,
        };
        let span = l.row_span(mid).unwrap();
        assert_eq!(span, r.start as usize..r.end as usize, "row {ri}");
    }
    assert_caret_round_trip(&l, pi);
    assert_selection_covers_each_glyph_once(&l, pi);
}

#[test]
fn tag_chips_stay_whole_in_right_to_left_text() {
    let l = layout_of(
        "<doc><p>שלום <b>עולם</b> טוב</p></doc>",
        ViewMode::TagsOn,
        600.0,
    );
    let pi = rtl_paras(&l)[0];
    let p = &l.paras[pi];
    let map = p.bidi.as_ref().unwrap();
    assert!(
        map.base_rtl,
        "a chip at the start does not decide the direction"
    );
    // Each chip's glyphs are next to each other, in order, left to right.
    for seg in p.segs.iter() {
        if !matches!(seg.kind, SegKind::StartTag(_) | SegKind::EndTag(_)) {
            continue;
        }
        let g: Vec<u32> = seg.chars.clone().map(|i| map.glyph[i]).collect();
        assert!(g.windows(2).all(|w| w[1] == w[0] + 1), "chip glyphs {g:?}");
    }
    assert_caret_round_trip(&l, pi);
    assert_selection_covers_each_glyph_once(&l, pi);
}

#[test]
fn arrow_keys_reverse_inside_right_to_left_text() {
    let l = layout_of("<doc><p>abc שלום</p></doc>", ViewMode::Normal, 600.0);
    let pi = rtl_paras(&l)[0];
    assert!(!l.rtl_at(Loc { para: pi, ch: 1 }));
    assert!(l.rtl_at(Loc { para: pi, ch: 6 }));
}
