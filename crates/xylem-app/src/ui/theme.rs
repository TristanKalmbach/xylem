//! Colours, type and spacing for the application chrome.
//!
//! Everything the editor draws around the document (panels, toolbars,
//! dialogs, chips, gutters) takes its colours from a [`Palette`], so the two
//! themes stay consistent and a change of accent is a one-line edit. The
//! document's own look is not themed here: that comes from the doctype
//! package's stylesheet.
//!
//! The palette is stored in the egui context; read it with [`palette`].

use egui::{Color32, CornerRadius, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle, Vec2};

/// Semantic colours for one theme.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Palette {
    pub dark: bool,
    /// Window background behind the panels (menu bar, toolbar, status bar).
    pub chrome: Color32,
    /// Side panels.
    pub panel: Color32,
    /// The document area, text fields and list backgrounds.
    pub surface: Color32,
    /// Dialogs, menus and popups.
    pub raised: Color32,
    /// Hovered rows and frameless buttons.
    pub hover: Color32,
    /// Pressed buttons.
    pub pressed: Color32,
    pub border: Color32,
    pub border_strong: Color32,
    pub text: Color32,
    /// Secondary text: labels, hints, metadata.
    pub text_muted: Color32,
    /// Placeholder text and disabled controls.
    pub text_faint: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    /// Text drawn on an accent fill.
    pub on_accent: Color32,
    /// Selected rows, the active segment, text selection.
    pub accent_soft: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub danger: Color32,
    /// Soft backgrounds behind status messages.
    pub success_soft: Color32,
    pub warning_soft: Color32,
    pub danger_soft: Color32,
}

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

impl Palette {
    pub const LIGHT: Palette = Palette {
        dark: false,
        chrome: rgb(0xECEFED),
        panel: rgb(0xF5F7F5),
        surface: rgb(0xFFFFFF),
        raised: rgb(0xFFFFFF),
        hover: rgb(0xE3E8E5),
        pressed: rgb(0xD6DDD9),
        border: rgb(0xD5DCD8),
        border_strong: rgb(0xB9C3BE),
        text: rgb(0x1B2224),
        text_muted: rgb(0x535E60),
        text_faint: rgb(0x7E898B),
        accent: rgb(0x16705F),
        accent_hover: rgb(0x105C4E),
        on_accent: rgb(0xFFFFFF),
        accent_soft: rgb(0xD3EBE4),
        success: rgb(0x1E7445),
        warning: rgb(0x8F5A00),
        danger: rgb(0xB0281F),
        success_soft: rgb(0xDDF1E4),
        warning_soft: rgb(0xFBEBCB),
        danger_soft: rgb(0xFADDDA),
    };

    pub const DARK: Palette = Palette {
        dark: true,
        chrome: rgb(0x151819),
        panel: rgb(0x1B1F20),
        surface: rgb(0x202526),
        raised: rgb(0x262B2D),
        hover: rgb(0x2B3234),
        pressed: rgb(0x343C3E),
        border: rgb(0x31383A),
        border_strong: rgb(0x46504F),
        text: rgb(0xE3E7E6),
        text_muted: rgb(0xA4ADAD),
        text_faint: rgb(0x778182),
        accent: rgb(0x4DBEA6),
        accent_hover: rgb(0x68CDB8),
        on_accent: rgb(0x0B1916),
        accent_soft: rgb(0x1C4740),
        success: rgb(0x5CC58C),
        warning: rgb(0xE2AC45),
        danger: rgb(0xF07A70),
        success_soft: rgb(0x1D3A2A),
        warning_soft: rgb(0x40321A),
        danger_soft: rgb(0x46231F),
    };

    pub fn for_dark_mode(dark: bool) -> Self {
        if dark { Self::DARK } else { Self::LIGHT }
    }
}

/// Spacing scale (points). Everything lines up on a 4-point grid.
pub mod space {
    pub const S: f32 = 4.0;
    pub const M: f32 = 8.0;
    pub const L: f32 = 12.0;
    pub const XL: f32 = 16.0;
    pub const XXL: f32 = 24.0;
}

/// Corner radii.
pub mod radius {
    pub const CONTROL: u8 = 6;
    pub const CARD: u8 = 8;
    pub const WINDOW: u8 = 10;
}

/// Font sizes.
pub mod size {
    pub const SMALL: f32 = 11.5;
    pub const BODY: f32 = 13.5;
    pub const MONO: f32 = 13.0;
    /// Section labels in panels ("ELEMENTS").
    pub const SECTION: f32 = 11.0;
    /// Dialog titles.
    pub const TITLE: f32 = 15.5;
    pub const HEADING: f32 = 20.0;
    pub const ICON: f32 = 16.0;
}

/// Font family for headings and titles (semibold when the system has one).
pub fn strong_family() -> FontFamily {
    FontFamily::Name("ui-strong".into())
}

/// Text style for panel section labels.
pub fn section_style() -> TextStyle {
    TextStyle::Name("section".into())
}

/// Text style for dialog titles.
pub fn title_style() -> TextStyle {
    TextStyle::Name("title".into())
}

fn palette_id() -> egui::Id {
    egui::Id::new("xylem-palette")
}

/// The palette in use.
pub fn palette(ctx: &egui::Context) -> Palette {
    ctx.data(|d| d.get_temp(palette_id()))
        .unwrap_or_else(|| Palette::for_dark_mode(ctx.style().visuals.dark_mode))
}

/// Applies the theme for `dark` to the whole UI.
pub fn apply(ctx: &egui::Context, dark: bool) {
    let p = Palette::for_dark_mode(dark);
    ctx.data_mut(|d| d.insert_temp(palette_id(), p));
    ctx.set_theme(if dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
    let theme = if dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    };
    ctx.style_mut_of(theme, |style| configure(style, &p));
}

fn configure(style: &mut egui::Style, p: &Palette) {
    use TextStyle::*;
    style.text_styles = [
        (Small, FontId::proportional(size::SMALL)),
        (Body, FontId::proportional(size::BODY)),
        (Button, FontId::proportional(size::BODY)),
        (Monospace, FontId::monospace(size::MONO)),
        (Heading, FontId::new(size::HEADING, strong_family())),
        (section_style(), FontId::new(size::SECTION, strong_family())),
        (title_style(), FontId::new(size::TITLE, strong_family())),
    ]
    .into();

    let s = &mut style.spacing;
    s.item_spacing = Vec2::new(space::M, 6.0);
    s.button_padding = Vec2::new(10.0, 4.0);
    s.interact_size = Vec2::new(28.0, 26.0);
    s.window_margin = Margin::same(space::XL as i8);
    s.menu_margin = Margin::same(6);
    s.indent = 16.0;
    s.icon_width = 15.0;
    s.icon_width_inner = 9.0;
    s.icon_spacing = 6.0;
    s.combo_width = 140.0;
    s.text_edit_width = 220.0;
    s.tooltip_width = 360.0;

    let v = &mut style.visuals;
    *v = if p.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.panel_fill = p.panel;
    v.window_fill = p.raised;
    v.window_stroke = Stroke::new(1.0_f32, p.border);
    v.window_corner_radius = CornerRadius::same(radius::WINDOW);
    v.menu_corner_radius = CornerRadius::same(radius::CARD);
    v.window_shadow = Shadow {
        offset: [0, 8],
        blur: 28,
        spread: 0,
        color: Color32::from_black_alpha(if p.dark { 110 } else { 38 }),
    };
    v.popup_shadow = Shadow {
        offset: [0, 4],
        blur: 14,
        spread: 0,
        color: Color32::from_black_alpha(if p.dark { 90 } else { 30 }),
    };
    v.window_highlight_topmost = false;
    v.faint_bg_color = p.hover;
    v.extreme_bg_color = p.surface;
    v.text_edit_bg_color = Some(p.surface);
    v.code_bg_color = p.hover;
    v.hyperlink_color = p.accent;
    v.warn_fg_color = p.warning;
    v.error_fg_color = p.danger;
    v.override_text_color = None;
    v.weak_text_color = Some(p.text_muted);
    v.selection.bg_fill = p.accent_soft;
    v.selection.stroke = Stroke::new(1.0_f32, p.accent);
    v.text_cursor.stroke = Stroke::new(2.0_f32, p.accent);
    v.indent_has_left_vline = false;
    v.striped = false;
    v.collapsing_header_frame = false;
    v.button_frame = true;

    let control = CornerRadius::same(radius::CONTROL);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.panel;
    w.noninteractive.weak_bg_fill = p.panel;
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.text);
    w.noninteractive.corner_radius = control;

    w.inactive.bg_fill = p.hover;
    w.inactive.weak_bg_fill = p.hover;
    // Text fields draw their edge from this; on white dialogs a field
    // without one is invisible.
    w.inactive.bg_stroke = Stroke::new(1.0_f32, p.border);
    w.inactive.fg_stroke = Stroke::new(1.0_f32, p.text);
    w.inactive.corner_radius = control;
    w.inactive.expansion = 0.0;

    w.hovered.bg_fill = p.pressed;
    w.hovered.weak_bg_fill = p.pressed;
    w.hovered.bg_stroke = Stroke::new(1.0_f32, p.border_strong);
    w.hovered.fg_stroke = Stroke::new(1.0_f32, p.text);
    w.hovered.corner_radius = control;
    w.hovered.expansion = 0.0;

    w.active.bg_fill = p.pressed;
    w.active.weak_bg_fill = p.pressed;
    w.active.bg_stroke = Stroke::new(1.0_f32, p.accent);
    w.active.fg_stroke = Stroke::new(1.0_f32, p.text);
    w.active.corner_radius = control;
    w.active.expansion = 0.0;

    w.open.bg_fill = p.hover;
    w.open.weak_bg_fill = p.hover;
    w.open.bg_stroke = Stroke::new(1.0_f32, p.border_strong);
    w.open.fg_stroke = Stroke::new(1.0_f32, p.text);
    w.open.corner_radius = control;
}

/// WCAG contrast ratio between two opaque colours (used by the tests that
/// keep both themes readable).
#[cfg(test)]
pub fn contrast(a: Color32, b: Color32) -> f32 {
    fn lum(c: Color32) -> f32 {
        let ch = |v: u8| {
            let v = v as f32 / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
    }
    let (la, lb) = (lum(a), lum(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every text colour must stay readable on every background it is used
    /// on, in both themes: 7:1 for body text, 4.5:1 for secondary text and
    /// accent text, 3:1 for placeholders (WCAG AA/AAA).
    #[test]
    fn text_contrast_in_both_themes() {
        for p in [Palette::LIGHT, Palette::DARK] {
            let theme = if p.dark { "dark" } else { "light" };
            for (bg_name, bg) in [
                ("chrome", p.chrome),
                ("panel", p.panel),
                ("surface", p.surface),
                ("raised", p.raised),
            ] {
                let check = |name: &str, fg: Color32, min: f32| {
                    let c = contrast(fg, bg);
                    assert!(
                        c >= min,
                        "{theme}: {name} on {bg_name} is {c:.2}:1, needs {min}:1"
                    );
                };
                check("text", p.text, 7.0);
                check("text_muted", p.text_muted, 4.5);
                check("text_faint", p.text_faint, 3.0);
                check("accent", p.accent, 4.5);
                check("danger", p.danger, 4.5);
                check("warning", p.warning, 4.5);
                check("success", p.success, 4.5);
            }
            let on = contrast(p.on_accent, p.accent);
            assert!(on >= 4.5, "{theme}: text on accent is {on:.2}:1");
            let sel = contrast(p.text, p.accent_soft);
            assert!(sel >= 7.0, "{theme}: text on selection is {sel:.2}:1");
            let hov = contrast(p.text, p.hover);
            assert!(hov >= 7.0, "{theme}: text on hover is {hov:.2}:1");
            for (name, fg, bg) in [
                ("danger on danger_soft", p.danger, p.danger_soft),
                ("warning on warning_soft", p.warning, p.warning_soft),
                ("success on success_soft", p.success, p.success_soft),
            ] {
                let c = contrast(fg, bg);
                assert!(c >= 4.5, "{theme}: {name} is {c:.2}:1");
            }
        }
    }
}
