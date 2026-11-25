//! Right-to-left text for the document view.
//!
//! egui draws one glyph per character, left to right, with no shaping.
//! Arabic and Hebrew need two things it does not do:
//!
//! - **Shaping.** Arabic letters take an isolated, initial, medial or final
//!   form depending on their neighbours, and lam followed by alef becomes
//!   one ligature. Unicode encodes those forms (Arabic Presentation
//!   Forms-A and -B), and the fonts Xylem uses map them, so shaping here is
//!   a character substitution in logical order: [`shape_arabic`].
//! - **Ordering.** The Unicode bidirectional algorithm (UAX #9) decides
//!   which runs read right to left. Levels are resolved once for the whole
//!   paragraph ([`resolve`]), then each wrapped line is put in visual order
//!   on its own ([`visual_order`]), with combining marks kept on their base
//!   letter (rule L3) and paired brackets mirrored (rule L4).
//!
//! Tag chips, comments and other atoms are single neutral objects in the
//! algorithm (one U+FFFC each), so a chip neither decides the paragraph's
//! direction nor gets reordered inside.
//!
//! Nothing here depends on egui; the layout uses it only for paragraphs
//! that contain right-to-left text.

use std::ops::Range;

use unicode_bidi::{BidiClass, BidiInfo, bidi_class};

/// Whether `text` has characters that read right to left (Hebrew, Arabic,
/// or an explicit right-to-left mark or embedding).
pub fn has_rtl(text: &str) -> bool {
    text.chars().any(|c| {
        // Cheap test first: everything below U+0590 is left to right.
        (c as u32) >= 0x0590
            && matches!(
                bidi_class(c),
                BidiClass::R | BidiClass::AL | BidiClass::RLE | BidiClass::RLO | BidiClass::RLI
            )
    })
}

// ---------------------------------------------------------------------------
// Arabic shaping
// ---------------------------------------------------------------------------

/// How a letter joins its neighbours (Unicode `Joining_Type`).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Join {
    /// Joins on both sides (most letters).
    Dual,
    /// Joins only to the letter before it (alef, dal, reh, waw, ...).
    Right,
    /// Joins on both sides without changing shape (tatweel, ZWJ).
    Causing,
    /// Marks: ignored when deciding how their neighbours join.
    Transparent,
    /// Does not join.
    None,
}

/// Presentation forms of a letter: isolated, final, initial, medial
/// (0 where the letter has no such form).
type Forms = [u16; 4];

/// The letters with presentation forms, by code point.
const FORMS: &[(u16, Forms)] = &[
    (0x0621, [0xFE80, 0, 0, 0]),
    (0x0622, [0xFE81, 0xFE82, 0, 0]),
    (0x0623, [0xFE83, 0xFE84, 0, 0]),
    (0x0624, [0xFE85, 0xFE86, 0, 0]),
    (0x0625, [0xFE87, 0xFE88, 0, 0]),
    (0x0626, [0xFE89, 0xFE8A, 0xFE8B, 0xFE8C]),
    (0x0627, [0xFE8D, 0xFE8E, 0, 0]),
    (0x0628, [0xFE8F, 0xFE90, 0xFE91, 0xFE92]),
    (0x0629, [0xFE93, 0xFE94, 0, 0]),
    (0x062A, [0xFE95, 0xFE96, 0xFE97, 0xFE98]),
    (0x062B, [0xFE99, 0xFE9A, 0xFE9B, 0xFE9C]),
    (0x062C, [0xFE9D, 0xFE9E, 0xFE9F, 0xFEA0]),
    (0x062D, [0xFEA1, 0xFEA2, 0xFEA3, 0xFEA4]),
    (0x062E, [0xFEA5, 0xFEA6, 0xFEA7, 0xFEA8]),
    (0x062F, [0xFEA9, 0xFEAA, 0, 0]),
    (0x0630, [0xFEAB, 0xFEAC, 0, 0]),
    (0x0631, [0xFEAD, 0xFEAE, 0, 0]),
    (0x0632, [0xFEAF, 0xFEB0, 0, 0]),
    (0x0633, [0xFEB1, 0xFEB2, 0xFEB3, 0xFEB4]),
    (0x0634, [0xFEB5, 0xFEB6, 0xFEB7, 0xFEB8]),
    (0x0635, [0xFEB9, 0xFEBA, 0xFEBB, 0xFEBC]),
    (0x0636, [0xFEBD, 0xFEBE, 0xFEBF, 0xFEC0]),
    (0x0637, [0xFEC1, 0xFEC2, 0xFEC3, 0xFEC4]),
    (0x0638, [0xFEC5, 0xFEC6, 0xFEC7, 0xFEC8]),
    (0x0639, [0xFEC9, 0xFECA, 0xFECB, 0xFECC]),
    (0x063A, [0xFECD, 0xFECE, 0xFECF, 0xFED0]),
    (0x0641, [0xFED1, 0xFED2, 0xFED3, 0xFED4]),
    (0x0642, [0xFED5, 0xFED6, 0xFED7, 0xFED8]),
    (0x0643, [0xFED9, 0xFEDA, 0xFEDB, 0xFEDC]),
    (0x0644, [0xFEDD, 0xFEDE, 0xFEDF, 0xFEE0]),
    (0x0645, [0xFEE1, 0xFEE2, 0xFEE3, 0xFEE4]),
    (0x0646, [0xFEE5, 0xFEE6, 0xFEE7, 0xFEE8]),
    (0x0647, [0xFEE9, 0xFEEA, 0xFEEB, 0xFEEC]),
    (0x0648, [0xFEED, 0xFEEE, 0, 0]),
    (0x0649, [0xFEEF, 0xFEF0, 0, 0]),
    (0x064A, [0xFEF1, 0xFEF2, 0xFEF3, 0xFEF4]),
    // Persian and Urdu letters.
    (0x0671, [0xFB50, 0xFB51, 0, 0]),
    (0x0679, [0xFB66, 0xFB67, 0xFB68, 0xFB69]),
    (0x067E, [0xFB56, 0xFB57, 0xFB58, 0xFB59]),
    (0x0686, [0xFB7A, 0xFB7B, 0xFB7C, 0xFB7D]),
    (0x0688, [0xFB88, 0xFB89, 0, 0]),
    (0x0691, [0xFB8C, 0xFB8D, 0, 0]),
    (0x0698, [0xFB8A, 0xFB8B, 0, 0]),
    (0x06A9, [0xFB8E, 0xFB8F, 0xFB90, 0xFB91]),
    (0x06AF, [0xFB92, 0xFB93, 0xFB94, 0xFB95]),
    (0x06BA, [0xFB9E, 0xFB9F, 0, 0]),
    (0x06BE, [0xFBAA, 0xFBAB, 0xFBAC, 0xFBAD]),
    (0x06C1, [0xFBA6, 0xFBA7, 0xFBA8, 0xFBA9]),
    (0x06CC, [0xFBFC, 0xFBFD, 0xFBFE, 0xFBFF]),
    (0x06D2, [0xFBAE, 0xFBAF, 0, 0]),
    (0x06D3, [0xFBB0, 0xFBB1, 0, 0]),
];

const LAM: char = '\u{0644}';

/// Lam-alef ligatures, isolated and final, for each alef.
const LAM_ALEF: &[(char, [u16; 2])] = &[
    ('\u{0622}', [0xFEF5, 0xFEF6]),
    ('\u{0623}', [0xFEF7, 0xFEF8]),
    ('\u{0625}', [0xFEF9, 0xFEFA]),
    ('\u{0627}', [0xFEFB, 0xFEFC]),
];

fn forms(c: char) -> Option<&'static Forms> {
    let cp = u16::try_from(c as u32).ok()?;
    FORMS
        .binary_search_by_key(&cp, |&(k, _)| k)
        .ok()
        .map(|i| &FORMS[i].1)
}

fn join(c: char) -> Join {
    if is_mark(c) {
        return Join::Transparent;
    }
    match c {
        '\u{0640}' | '\u{200D}' => Join::Causing,
        _ => match forms(c) {
            Some(f) if f[2] != 0 => Join::Dual,
            Some(f) if f[1] != 0 => Join::Right,
            _ => Join::None,
        },
    }
}

/// A non-spacing mark (harakat, niqqud, and other combining marks): it
/// belongs to the letter before it.
pub fn is_mark(c: char) -> bool {
    (c as u32) >= 0x0300 && bidi_class(c) == BidiClass::NSM
}

/// Text after shaping.
#[derive(Debug, Clone, PartialEq)]
pub struct Shaped {
    pub chars: Vec<char>,
    /// For each input character, the shaped character that shows it. A
    /// lam-alef ligature shows two input characters.
    pub of_input: Vec<u32>,
}

/// Replaces Arabic letters with their contextual forms. `keep[i]` false
/// leaves character `i` alone and breaks joining there (tag chips, atoms);
/// `has_glyph` says whether the font can show a form, so a form the font
/// lacks falls back to the plain letter.
pub fn shape_arabic(
    input: &[char],
    keep: impl Fn(usize) -> bool,
    mut has_glyph: impl FnMut(usize, char) -> bool,
) -> Shaped {
    let n = input.len();
    let kind: Vec<Join> = (0..n)
        .map(|i| if keep(i) { join(input[i]) } else { Join::None })
        .collect();
    // The nearest non-transparent neighbour on each side.
    let neighbour = |i: usize, step: isize| -> Option<usize> {
        let mut j = i as isize + step;
        while j >= 0 && (j as usize) < n {
            if kind[j as usize] != Join::Transparent {
                return Some(j as usize);
            }
            j += step;
        }
        None
    };
    let joins_back = |i: usize| -> bool {
        matches!(kind[i], Join::Dual | Join::Right | Join::Causing)
            && neighbour(i, -1).is_some_and(|p| matches!(kind[p], Join::Dual | Join::Causing))
    };
    let joins_forward = |i: usize| -> bool {
        matches!(kind[i], Join::Dual | Join::Causing)
            && neighbour(i, 1)
                .is_some_and(|q| matches!(kind[q], Join::Dual | Join::Right | Join::Causing))
    };
    let mut chars = Vec::with_capacity(n);
    let mut of_input = vec![0u32; n];
    let mut i = 0;
    while i < n {
        let c = input[i];
        of_input[i] = chars.len() as u32;
        // Lam directly followed by an alef: one ligature for both.
        if c == LAM
            && kind[i] == Join::Dual
            && i + 1 < n
            && let Some(&(_, lig)) = LAM_ALEF.iter().find(|(a, _)| *a == input[i + 1])
            && kind[i + 1] == Join::Right
        {
            let form = lig[usize::from(joins_back(i))];
            if let Some(l) = char::from_u32(u32::from(form))
                && has_glyph(i, l)
            {
                chars.push(l);
                of_input[i + 1] = of_input[i];
                i += 2;
                continue;
            }
        }
        let shaped = forms(c).filter(|_| keep(i)).and_then(|f| {
            let index = match (joins_back(i), joins_forward(i)) {
                (false, false) => 0,
                (true, false) => 1,
                (false, true) => 2,
                (true, true) => 3,
            };
            let form = if f[index] != 0 { f[index] } else { f[0] };
            char::from_u32(u32::from(form)).filter(|&s| has_glyph(i, s))
        });
        chars.push(shaped.unwrap_or(c));
        i += 1;
    }
    Shaped { chars, of_input }
}

// ---------------------------------------------------------------------------
// Bidirectional levels and line reordering
// ---------------------------------------------------------------------------

/// Embedding levels for every character of a paragraph.
#[derive(Debug, Clone, PartialEq)]
pub struct Levels {
    /// Resolved level of each character; odd reads right to left.
    pub levels: Vec<u8>,
    /// Level of the (bidi) paragraph each character is in.
    pub base: Vec<u8>,
}

impl Levels {
    pub fn rtl(&self, i: usize) -> bool {
        self.levels[i] % 2 == 1
    }
}

/// Resolves levels for `text` once, for the whole paragraph. Characters in
/// `objects` (sorted, non-overlapping ranges) count as one neutral object
/// each and all take that object's level.
pub fn resolve(text: &[char], objects: &[Range<usize>]) -> Levels {
    // One unit per character, or per object.
    let mut s = String::with_capacity(text.len() * 2);
    let mut unit_byte = Vec::with_capacity(text.len());
    let mut obj = objects.iter().peekable();
    let mut i = 0;
    while i < text.len() {
        while obj.next_if(|o| o.end <= i).is_some() {}
        if let Some(o) = obj.peek()
            && o.contains(&i)
        {
            let at = s.len();
            s.push('\u{FFFC}');
            for _ in o.start.max(i)..o.end {
                unit_byte.push(at);
            }
            i = o.end;
            continue;
        }
        unit_byte.push(s.len());
        s.push(text[i]);
        i += 1;
    }
    let info = BidiInfo::new(&s, None);
    let base_at = |byte: usize| -> u8 {
        info.paragraphs
            .iter()
            .find(|p| p.range.contains(&byte))
            .map_or(0, |p| p.level.number())
    };
    let levels = unit_byte.iter().map(|&b| info.levels[b].number()).collect();
    let base = unit_byte.iter().map(|&b| base_at(b)).collect();
    Levels { levels, base }
}

/// Visual order of the characters `line` (a range of the paragraph), left
/// to right, as paragraph indices. Objects and letter-plus-marks clusters
/// move as units and keep their inside order.
pub fn visual_order(
    text: &[char],
    levels: &Levels,
    objects: &[Range<usize>],
    line: Range<usize>,
) -> Vec<usize> {
    if line.is_empty() {
        return Vec::new();
    }
    // Units: (range, level).
    let mut units: Vec<(Range<usize>, u8)> = Vec::new();
    let mut i = line.start;
    while i < line.end {
        if let Some(o) = objects.iter().find(|o| o.contains(&i)) {
            let end = o.end.min(line.end);
            units.push((i..end, levels.levels[i]));
            i = end;
            continue;
        }
        let mut end = i + 1;
        while end < line.end && is_mark(text[end]) && !objects.iter().any(|o| o.contains(&end)) {
            end += 1;
        }
        units.push((i..end, levels.levels[i]));
        i = end;
    }
    // L1: whitespace at the end of the line takes the paragraph level.
    let base = levels.base[line.start];
    for u in units.iter_mut().rev() {
        let c = text[u.0.start];
        let ws = u.0.len() == 1
            && matches!(
                bidi_class(c),
                BidiClass::WS | BidiClass::S | BidiClass::B | BidiClass::BN
            );
        if !ws {
            break;
        }
        u.1 = base;
    }
    // L2: from the highest level down to the lowest odd level, reverse
    // every run at that level or above.
    let max = units.iter().map(|u| u.1).max().unwrap_or(0);
    let min_odd = units
        .iter()
        .map(|u| u.1)
        .filter(|l| l % 2 == 1)
        .min()
        .unwrap_or(max + 1);
    let mut level = max;
    while level >= min_odd && level > 0 {
        let mut k = 0;
        while k < units.len() {
            if units[k].1 >= level {
                let start = k;
                while k < units.len() && units[k].1 >= level {
                    k += 1;
                }
                units[start..k].reverse();
            } else {
                k += 1;
            }
        }
        level -= 1;
    }
    units.into_iter().flat_map(|(r, _)| r).collect()
}

/// A single line of plain text as it should be drawn: shaped and in visual
/// order. For read-only labels (the structure tree), which egui draws as
/// given.
pub fn display_line(text: &str) -> std::borrow::Cow<'_, str> {
    if !has_rtl(text) {
        return text.into();
    }
    let chars: Vec<char> = text.chars().collect();
    let shaped = shape_arabic(&chars, |_| true, |_, _| true).chars;
    let levels = resolve(&shaped, &[]);
    visual_order(&shaped, &levels, &[], 0..shaped.len())
        .into_iter()
        .map(|i| {
            if levels.rtl(i) {
                mirror(shaped[i])
            } else {
                shaped[i]
            }
        })
        .collect::<String>()
        .into()
}

/// The mirror image of a bracket, for characters shown right to left
/// (rule L4).
pub fn mirror(c: char) -> char {
    match c {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '{' => '}',
        '}' => '{',
        '<' => '>',
        '>' => '<',
        '«' => '»',
        '»' => '«',
        '‹' => '›',
        '›' => '‹',
        _ => c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(s: &str) -> String {
        let chars: Vec<char> = s.chars().collect();
        shape_arabic(&chars, |_| true, |_, _| true)
            .chars
            .into_iter()
            .collect()
    }

    /// A paragraph in visual order, as a string, the way it is drawn.
    fn visual(s: &str) -> String {
        visual_with(s, &[])
    }

    fn visual_with(s: &str, objects: &[Range<usize>]) -> String {
        let chars: Vec<char> = s.chars().collect();
        let levels = resolve(&chars, objects);
        visual_order(&chars, &levels, objects, 0..chars.len())
            .into_iter()
            .map(|i| {
                // Objects are drawn as they are; only text is mirrored.
                if levels.rtl(i) && !objects.iter().any(|o| o.contains(&i)) {
                    mirror(chars[i])
                } else {
                    chars[i]
                }
            })
            .collect()
    }

    #[test]
    fn labels_are_shaped_and_ordered() {
        assert_eq!(display_line("plain"), "plain");
        assert_eq!(display_line("The pump (משאבה)"), "The pump (הבאשמ)");
        assert_eq!(display_line("بيت"), "\u{FE96}\u{FEF4}\u{FE91}");
    }

    #[test]
    fn detects_right_to_left_text() {
        assert!(!has_rtl("Plain text, café, 中文"));
        assert!(has_rtl("Shalom שלום"));
        assert!(has_rtl("سلام"));
        assert!(has_rtl("\u{202B}x"));
    }

    #[test]
    fn letters_take_their_contextual_forms() {
        // beh initial, yeh medial, teh final.
        assert_eq!(shape("بيت"), "\u{FE91}\u{FEF4}\u{FE96}");
        // seen initial, then the lam-alef ligature in its final form, then
        // meem isolated (alef does not join forward).
        assert_eq!(shape("سلام"), "\u{FEB3}\u{FEFC}\u{FEE1}");
        // A lone letter, and one at each end of a word.
        assert_eq!(shape("ب"), "\u{FE8F}");
        assert_eq!(shape("بب"), "\u{FE91}\u{FE90}");
        // Hamza never joins.
        assert_eq!(shape("بءب"), "\u{FE8F}\u{FE80}\u{FE8F}");
        // Marks are skipped when deciding joins, and kept.
        assert_eq!(shape("بَب"), "\u{FE91}\u{064E}\u{FE90}");
        // Tatweel joins on both sides.
        assert_eq!(shape("بـب"), "\u{FE91}\u{0640}\u{FE90}");
        // Persian letters.
        assert_eq!(shape("پی"), "\u{FB58}\u{FBFD}");
        // Latin and Hebrew pass through.
        assert_eq!(shape("ab שלום"), "ab שלום");
    }

    #[test]
    fn lam_alef_is_one_ligature() {
        let chars: Vec<char> = "لا علا".chars().collect();
        let s = shape_arabic(&chars, |_| true, |_, _| true);
        let text: String = s.chars.iter().collect();
        // Isolated after a space, final after ain.
        assert_eq!(text, "\u{FEFB} \u{FECB}\u{FEFC}");
        // Lam and alef both map to the ligature.
        assert_eq!(s.of_input, vec![0, 0, 1, 2, 3, 3]);
    }

    #[test]
    fn missing_glyphs_fall_back_to_the_letter() {
        let chars: Vec<char> = "بيت".chars().collect();
        let s = shape_arabic(&chars, |_| true, |_, c| c != '\u{FEF4}');
        assert_eq!(s.chars[1], 'ي');
        let s = shape_arabic(&chars, |_| true, |_, _| false);
        assert_eq!(s.chars, chars);
    }

    #[test]
    fn objects_break_joining_and_are_left_alone() {
        let chars: Vec<char> = "بxب".chars().collect();
        let s = shape_arabic(&chars, |i| i != 1, |_, _| true);
        assert_eq!(s.chars, vec!['\u{FE8F}', 'x', '\u{FE8F}']);
    }

    #[test]
    fn hebrew_reads_right_to_left() {
        assert_eq!(visual("שלום"), "םולש");
        // In a left-to-right paragraph, the Hebrew word turns in place.
        assert_eq!(visual("abc שלום def"), "abc םולש def");
        // In a right-to-left paragraph, numbers and Latin stay left to right.
        assert_eq!(visual("שלום 123"), "123 םולש");
        assert_eq!(visual("שלום abc def"), "abc def םולש");
    }

    #[test]
    fn brackets_are_mirrored_in_right_to_left_runs() {
        assert_eq!(visual("(שלום)"), "(םולש)");
        assert_eq!(visual("a (b) c"), "a (b) c");
    }

    #[test]
    fn marks_stay_on_their_letter() {
        // shin + qamats + dagesh-less shin dot, lamed, vav + holam, final mem.
        let word = "שָׁלוֹם";
        let v = visual(word);
        let chars: Vec<char> = v.chars().collect();
        assert_eq!(chars[0], 'ם');
        // The marks follow their base letter in the visual string too.
        assert_eq!(
            chars,
            vec!['ם', 'ו', '\u{05B9}', 'ל', 'ש', '\u{05B8}', '\u{05C1}']
        );
    }

    #[test]
    fn objects_are_neutral_and_keep_their_order() {
        // A chip at the start does not make the paragraph left to right,
        // and its own letters are not reversed.
        let s = "<p>שלום";
        let v = visual_with(s, &[Range { start: 0, end: 3 }]);
        assert_eq!(v, "םולש<p>");
        // Two chips side by side stay in order inside a right-to-left run.
        let s = "אב</b><i>גד";
        let v = visual_with(s, &[2..6, 6..9]);
        assert_eq!(v, "דג<i></b>בא");
    }

    #[test]
    fn trailing_spaces_take_the_paragraph_level() {
        let chars: Vec<char> = "abc שלום ".chars().collect();
        let levels = resolve(&chars, &[]);
        let order = visual_order(&chars, &levels, &[], 0..chars.len());
        // The trailing space stays at the right end of a left-to-right line.
        assert_eq!(*order.last().unwrap(), chars.len() - 1);
    }

    #[test]
    fn lines_are_reordered_separately() {
        // One right-to-left paragraph wrapped after the first word: each line
        // is reversed on its own, with levels from the whole paragraph.
        let chars: Vec<char> = "אבג דהו".chars().collect();
        let levels = resolve(&chars, &[]);
        let first = visual_order(&chars, &levels, &[], 0..4);
        let second = visual_order(&chars, &levels, &[], 4..7);
        // The first line's trailing space is at the paragraph level (right
        // to left), so it ends up at the left end of the line.
        assert_eq!(first, vec![3, 2, 1, 0]);
        assert_eq!(second, vec![6, 5, 4]);
    }
}
