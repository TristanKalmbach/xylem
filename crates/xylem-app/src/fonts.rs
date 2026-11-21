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
    ctx.set_fonts(defs);
    FontSet {
        has_bold: loaded.contains("sans-b"),
        has_italic: loaded.contains("sans-i"),
        has_serif: loaded.contains("serif"),
    }
}
