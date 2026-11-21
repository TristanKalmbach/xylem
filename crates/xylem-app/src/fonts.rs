//! Loads system fonts so documents can use real bold/italic/serif/mono faces.

use crate::view::layout::FontSet;
use egui::{FontData, FontDefinitions, FontFamily};
use std::path::PathBuf;
use std::sync::Arc;

fn font_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if cfg!(windows) {
        let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
        v.push(PathBuf::from(windir).join("Fonts"));
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            v.push(PathBuf::from(local).join("Microsoft\\Windows\\Fonts"));
        }
    } else if cfg!(target_os = "macos") {
        v.push("/System/Library/Fonts".into());
        v.push("/System/Library/Fonts/Supplemental".into());
        v.push("/Library/Fonts".into());
        if let Some(h) = std::env::var_os("HOME") {
            v.push(PathBuf::from(h).join("Library/Fonts"));
        }
    } else {
        v.push("/usr/share/fonts".into());
        v.push("/usr/local/share/fonts".into());
        if let Some(h) = std::env::var_os("HOME") {
            v.push(PathBuf::from(&h).join(".fonts"));
            v.push(PathBuf::from(h).join(".local/share/fonts"));
        }
    }
    v
}

fn find(names: &[&str]) -> Option<Vec<u8>> {
    let dirs = font_dirs();
    for name in names {
        for d in &dirs {
            let direct = d.join(name);
            if direct.is_file()
                && let Ok(b) = std::fs::read(&direct)
            {
                return Some(b);
            }
        }
    }
    // Recursive search (Linux font trees are nested).
    for d in &dirs {
        if let Some(p) = search(d, names, 4)
            && let Ok(b) = std::fs::read(p)
        {
            return Some(b);
        }
    }
    None
}

fn search(dir: &std::path::Path, names: &[&str], depth: usize) -> Option<PathBuf> {
    if depth == 0 {
        return None;
    }
    let rd = std::fs::read_dir(dir).ok()?;
    let mut subdirs = Vec::new();
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            subdirs.push(p);
        } else if let Some(f) = p.file_name().and_then(|f| f.to_str())
            && names.iter().any(|n| n.eq_ignore_ascii_case(f))
        {
            return Some(p);
        }
    }
    subdirs
        .into_iter()
        .find_map(|s| search(&s, names, depth - 1))
}

struct Face {
    family: &'static str,
    candidates: &'static [&'static str],
}

const FACES: &[Face] = &[
    Face {
        family: "sans",
        candidates: &[
            "segoeui.ttf",
            "Arial.ttf",
            "arial.ttf",
            "DejaVuSans.ttf",
            "LiberationSans-Regular.ttf",
            "NotoSans-Regular.ttf",
            "Helvetica.ttc",
        ],
    },
    Face {
        family: "sans-b",
        candidates: &[
            "segoeuib.ttf",
            "Arial Bold.ttf",
            "arialbd.ttf",
            "DejaVuSans-Bold.ttf",
            "LiberationSans-Bold.ttf",
            "NotoSans-Bold.ttf",
        ],
    },
    Face {
        family: "sans-i",
        candidates: &[
            "segoeuii.ttf",
            "Arial Italic.ttf",
            "ariali.ttf",
            "DejaVuSans-Oblique.ttf",
            "LiberationSans-Italic.ttf",
            "NotoSans-Italic.ttf",
        ],
    },
    Face {
        family: "sans-bi",
        candidates: &[
            "segoeuiz.ttf",
            "Arial Bold Italic.ttf",
            "arialbi.ttf",
            "DejaVuSans-BoldOblique.ttf",
            "LiberationSans-BoldItalic.ttf",
            "NotoSans-BoldItalic.ttf",
        ],
    },
    Face {
        family: "serif",
        candidates: &[
            "times.ttf",
            "Times New Roman.ttf",
            "DejaVuSerif.ttf",
            "LiberationSerif-Regular.ttf",
            "NotoSerif-Regular.ttf",
        ],
    },
    Face {
        family: "serif-b",
        candidates: &[
            "timesbd.ttf",
            "Times New Roman Bold.ttf",
            "DejaVuSerif-Bold.ttf",
            "LiberationSerif-Bold.ttf",
            "NotoSerif-Bold.ttf",
        ],
    },
    Face {
        family: "serif-i",
        candidates: &[
            "timesi.ttf",
            "Times New Roman Italic.ttf",
            "DejaVuSerif-Italic.ttf",
            "LiberationSerif-Italic.ttf",
            "NotoSerif-Italic.ttf",
        ],
    },
    Face {
        family: "serif-bi",
        candidates: &[
            "timesbi.ttf",
            "Times New Roman Bold Italic.ttf",
            "DejaVuSerif-BoldItalic.ttf",
            "LiberationSerif-BoldItalic.ttf",
            "NotoSerif-BoldItalic.ttf",
        ],
    },
    Face {
        family: "mono",
        candidates: &[
            "consola.ttf",
            "Menlo.ttc",
            "DejaVuSansMono.ttf",
            "LiberationMono-Regular.ttf",
            "cour.ttf",
        ],
    },
    Face {
        family: "mono-b",
        candidates: &[
            "consolab.ttf",
            "DejaVuSansMono-Bold.ttf",
            "LiberationMono-Bold.ttf",
            "courbd.ttf",
        ],
    },
    Face {
        family: "mono-i",
        candidates: &[
            "consolai.ttf",
            "DejaVuSansMono-Oblique.ttf",
            "LiberationMono-Italic.ttf",
            "couri.ttf",
        ],
    },
    Face {
        family: "mono-bi",
        candidates: &[
            "consolaz.ttf",
            "DejaVuSansMono-BoldOblique.ttf",
            "LiberationMono-BoldItalic.ttf",
            "courbi.ttf",
        ],
    },
];

/// Semibold faces for UI headings, by platform.
const STRONG_UI: &[&str] = &[
    "seguisb.ttf",
    "Segoe UI Semibold.ttf",
    "Inter-SemiBold.ttf",
    "NotoSans-SemiBold.ttf",
    "OpenSans-SemiBold.ttf",
    "Ubuntu-Medium.ttf",
];

/// Installs document font families; falls back to egui's built-in fonts.
pub fn install(ctx: &egui::Context) -> FontSet {
    let mut defs = FontDefinitions::default();
    let fallback_prop = defs
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let fallback_mono = defs
        .families
        .get(&FontFamily::Monospace)
        .cloned()
        .unwrap_or_default();
    let mut loaded = std::collections::HashSet::new();
    for face in FACES {
        let mut chain = Vec::new();
        if let Some(bytes) = find(face.candidates) {
            let key = format!("doc-{}", face.family);
            defs.font_data
                .insert(key.clone(), Arc::new(FontData::from_owned(bytes)));
            chain.push(key);
            loaded.insert(face.family);
        } else {
            // Variant missing: fall back to the regular face of the family.
            let base = face.family.split('-').next().unwrap();
            let key = format!("doc-{base}");
            if defs.font_data.contains_key(&key) {
                chain.push(key);
            }
        }
        let fb = if face.family.starts_with("mono") {
            &fallback_mono
        } else {
            &fallback_prop
        };
        chain.extend(fb.iter().cloned());
        defs.families
            .insert(FontFamily::Name(face.family.into()), chain);
    }
    // Use the system sans for the UI too when available (better coverage).
    if loaded.contains("sans")
        && let Some(p) = defs.families.get_mut(&FontFamily::Proportional)
    {
        p.insert(0, "doc-sans".into());
    }
    // Headings and titles: the system's semibold face, else its bold one.
    let mut strong = Vec::new();
    if let Some(bytes) = find(STRONG_UI) {
        defs.font_data
            .insert("ui-strong".into(), Arc::new(FontData::from_owned(bytes)));
        strong.push("ui-strong".to_string());
    } else if loaded.contains("sans-b") {
        strong.push("doc-sans-b".to_string());
    }
    // Icons (Phosphor, embedded) come right after the text face, so they
    // are used for the private-use code points the text font lacks.
    egui_phosphor::add_to_fonts(&mut defs, egui_phosphor::Variant::Regular);
    let prop = defs
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    strong.extend(prop);
    defs.families
        .insert(crate::ui::theme::strong_family(), strong);
    // Symbols most text fonts lack (arrows, maths, dingbats): small, so
    // always loaded.
    if let Some(bytes) = find(&[
        "seguisym.ttf",
        "NotoSansSymbols-Regular.ttf",
        "DejaVuSans.ttf",
    ]) {
        defs.font_data.insert(
            "fallback-symbols".into(),
            Arc::new(FontData::from_owned(bytes)),
        );
        for chain in defs.families.values_mut() {
            chain.push("fallback-symbols".into());
        }
    }
    if let Ok(mut base) = BASE.lock() {
        *base = Some(defs.clone());
    }
    ctx.set_fonts(defs);
    FontSet {
        has_bold: loaded.contains("sans-b"),
        has_italic: loaded.contains("sans-i"),
        has_serif: loaded.contains("serif"),
    }
}

// ------------------------------------------------------------------ scripts

/// The definitions installed at start-up, extended as documents need more
/// scripts.
static BASE: std::sync::Mutex<Option<FontDefinitions>> = std::sync::Mutex::new(None);

/// Writing systems that need fonts beyond the Latin ones loaded at start-up.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Script {
    Arabic,
    Hebrew,
    Thai,
    Indic,
    Japanese,
    Korean,
    ChineseSimplified,
    ChineseTraditional,
}

impl Script {
    pub fn name(self) -> &'static str {
        match self {
            Script::Arabic => "Arabic",
            Script::Hebrew => "Hebrew",
            Script::Thai => "Thai",
            Script::Indic => "Indic",
            Script::Japanese => "Japanese",
            Script::Korean => "Korean",
            Script::ChineseSimplified | Script::ChineseTraditional => "Chinese",
        }
    }

    /// Scripts whose letters change shape or order with their neighbours.
    /// The text layout draws them one character at a time, left to right,
    /// so they show but don't read correctly yet.
    pub fn needs_shaping(self) -> bool {
        matches!(
            self,
            Script::Arabic | Script::Hebrew | Script::Thai | Script::Indic
        )
    }

    fn fonts(self) -> &'static [&'static str] {
        match self {
            // Segoe UI and Arial already cover these on Windows.
            Script::Arabic => &[
                "segoeui.ttf",
                "arial.ttf",
                "NotoSansArabic-Regular.ttf",
                "DejaVuSans.ttf",
                "GeezaPro.ttc",
            ],
            Script::Hebrew => &[
                "segoeui.ttf",
                "arial.ttf",
                "NotoSansHebrew-Regular.ttf",
                "DejaVuSans.ttf",
                "ArialHB.ttc",
            ],
            Script::Thai => &[
                "LeelawUI.ttf",
                "tahoma.ttf",
                "NotoSansThai-Regular.ttf",
                "Thonburi.ttc",
            ],
            Script::Indic => &[
                "Nirmala.ttc",
                "Nirmala.ttf",
                "NotoSansDevanagari-Regular.ttf",
                "Kohinoor.ttc",
            ],
            Script::Japanese => &[
                "YuGothR.ttc",
                "YuGothM.ttc",
                "meiryo.ttc",
                "msgothic.ttc",
                "NotoSansCJK-Regular.ttc",
                "NotoSansCJKjp-Regular.otf",
                "Hiragino Sans GB.ttc",
            ],
            Script::Korean => &[
                "malgun.ttf",
                "NotoSansCJK-Regular.ttc",
                "NotoSansCJKkr-Regular.otf",
                "AppleSDGothicNeo.ttc",
            ],
            Script::ChineseSimplified => &[
                "msyh.ttc",
                "simsun.ttc",
                "NotoSansCJK-Regular.ttc",
                "NotoSansCJKsc-Regular.otf",
                "PingFang.ttc",
            ],
            Script::ChineseTraditional => &[
                "msjh.ttc",
                "mingliu.ttc",
                "NotoSansCJK-Regular.ttc",
                "NotoSansCJKtc-Regular.otf",
                "PingFang.ttc",
            ],
        }
    }
}

/// The scripts used in `text`. `traditional` picks Traditional Chinese
/// fonts for Han characters (from the document's `xml:lang`).
pub fn scripts_in(text: &str, traditional: bool) -> Vec<Script> {
    let (mut han, mut kana, mut hangul) = (false, false, false);
    let mut found = Vec::new();
    let add = |s: Script, found: &mut Vec<Script>| {
        if !found.contains(&s) {
            found.push(s);
        }
    };
    for c in text.chars() {
        let u = c as u32;
        if u < 0x0590 {
            continue;
        }
        match u {
            0x0590..=0x05FF | 0xFB1D..=0xFB4F => add(Script::Hebrew, &mut found),
            0x0600..=0x06FF
            | 0x0750..=0x077F
            | 0x08A0..=0x08FF
            | 0xFB50..=0xFDFF
            | 0xFE70..=0xFEFF => add(Script::Arabic, &mut found),
            0x0900..=0x0DFF => add(Script::Indic, &mut found),
            0x0E00..=0x0E7F => add(Script::Thai, &mut found),
            0x3040..=0x30FF | 0x31F0..=0x31FF => kana = true,
            0x1100..=0x11FF | 0x3130..=0x318F | 0xAC00..=0xD7AF => hangul = true,
            0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FFFF => han = true,
            _ => {}
        }
    }
    if kana {
        found.push(Script::Japanese);
    }
    if hangul {
        found.push(Script::Korean);
    }
    // Japanese fonts lack many Chinese characters, so Han text always gets
    // a Chinese font too (after the Japanese one when both are present).
    if han {
        found.push(if traditional {
            Script::ChineseTraditional
        } else {
            Script::ChineseSimplified
        });
    }
    found
}

/// Makes sure fonts for `scripts` are installed, adding them as fallbacks
/// to every family. Returns the scripts that were newly added (so callers
/// can lay text out again), and those no installed font covers.
pub fn ensure(ctx: &egui::Context, scripts: &[Script]) -> (Vec<Script>, Vec<Script>) {
    static LOADED: std::sync::Mutex<Vec<Script>> = std::sync::Mutex::new(Vec::new());
    let Ok(mut loaded) = LOADED.lock() else {
        return (Vec::new(), Vec::new());
    };
    let wanted: Vec<Script> = scripts
        .iter()
        .copied()
        .filter(|s| !loaded.contains(s))
        .collect();
    if wanted.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let Ok(mut base) = BASE.lock() else {
        return (Vec::new(), Vec::new());
    };
    let Some(defs) = base.as_mut() else {
        return (Vec::new(), Vec::new());
    };
    let (mut added, mut missing) = (Vec::new(), Vec::new());
    for s in wanted {
        loaded.push(s);
        let key = format!("script-{s:?}");
        if defs.font_data.contains_key(&key) {
            continue;
        }
        match find(s.fonts()) {
            Some(bytes) => {
                defs.font_data
                    .insert(key.clone(), Arc::new(FontData::from_owned(bytes)));
                for chain in defs.families.values_mut() {
                    chain.push(key.clone());
                }
                log::info!("loaded fonts for {s:?} text");
                added.push(s);
            }
            None => {
                log::warn!("no font found for {s:?} text");
                missing.push(s);
            }
        }
    }
    if !added.is_empty() {
        ctx.set_fonts(defs.clone());
    }
    (added, missing)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_are_recognised() {
        assert!(scripts_in("Plain English, café, naïve", false).is_empty());
        assert_eq!(
            scripts_in("保养手册", false),
            vec![Script::ChineseSimplified]
        );
        assert_eq!(
            scripts_in("保養手冊", true),
            vec![Script::ChineseTraditional]
        );
        assert_eq!(
            scripts_in("取扱説明書をお読みください", false),
            vec![Script::Japanese, Script::ChineseSimplified]
        );
        assert_eq!(scripts_in("사용 설명서", false), vec![Script::Korean]);
        assert_eq!(scripts_in("دليل المستخدم", false), vec![Script::Arabic]);
        assert_eq!(scripts_in("מדריך", false), vec![Script::Hebrew]);
        assert_eq!(scripts_in("คู่มือ", false), vec![Script::Thai]);
        assert_eq!(scripts_in("उपयोगकर्ता", false), vec![Script::Indic]);
        assert!(Script::Arabic.needs_shaping() && !Script::Japanese.needs_shaping());
    }
}
