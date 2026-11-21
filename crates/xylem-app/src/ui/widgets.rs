//! Shared UI components. Every panel and dialog builds on these, so the
//! application looks and behaves the same everywhere: buttons are the same
//! height, dialogs put their actions in the same place, Enter and Escape do
//! the same thing in every dialog.
//!
//! Each clickable component registers itself with the test harness under its
//! label (see [`crate::script::track`]), so scripted tests can find it
//! without screen coordinates.

use super::theme::{self, Palette, palette, radius, size, space};
use egui::{
    Align, Align2, Color32, CornerRadius, FontId, Frame, Layout, Margin, Rect, Response, RichText,
    Sense, Stroke, Ui, Vec2, WidgetText, vec2,
};

pub use egui_phosphor::regular as icon;

fn track(label: &str, r: &Response) {
    crate::script::track(label, r);
}

/// A compact, frameless button showing an icon. `tooltip` is also the
/// label scripts use; `shortcut` is shown in the tooltip.
pub fn icon_button(ui: &mut Ui, glyph: &str, tooltip: &str, shortcut: Option<&str>) -> Response {
    icon_toggle(ui, false, glyph, tooltip, shortcut)
}

/// Like [`icon_button`], drawn as pressed while `selected`.
pub fn icon_toggle(
    ui: &mut Ui,
    selected: bool,
    glyph: &str,
    tooltip: &str,
    shortcut: Option<&str>,
) -> Response {
    let p = palette(ui.ctx());
    let size = vec2(30.0, 28.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let enabled = ui.is_enabled();
    if ui.is_rect_visible(rect) {
        let fill = if selected {
            p.accent_soft
        } else if resp.is_pointer_button_down_on() {
            p.pressed
        } else if resp.hovered() && enabled {
            p.hover
        } else {
            Color32::TRANSPARENT
        };
        ui.painter()
            .rect_filled(rect, CornerRadius::same(radius::CONTROL), fill);
        let color = if !enabled {
            p.text_faint
        } else if selected {
            p.accent
        } else {
            p.text
        };
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            glyph,
            FontId::proportional(size::ICON + 1.0),
            color,
        );
    }
    let tip = match shortcut {
        Some(s) => format!("{tooltip}  ({s})"),
        None => tooltip.to_string(),
    };
    let resp = resp.on_hover_text(tip);
    track(tooltip, &resp);
    resp
}

/// A thin vertical divider between toolbar groups.
pub fn toolbar_divider(ui: &mut Ui) {
    let p = palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(9.0, 20.0), Sense::hover());
    ui.painter().vline(
        rect.center().x,
        rect.y_range(),
        Stroke::new(1.0_f32, p.border),
    );
}

/// The main action of a dialog or panel: accent fill.
pub fn primary_button(ui: &mut Ui, label: &str) -> Response {
    let p = palette(ui.ctx());
    let enabled = ui.is_enabled();
    let text = RichText::new(label).color(if enabled { p.on_accent } else { p.text_faint });
    let mut b = egui::Button::new(text).min_size(BUTTON_SIZE);
    if enabled {
        b = b.fill(p.accent);
    }
    let r = ui.add(b);
    track(label, &r);
    r
}

/// A normal button.
pub fn button(ui: &mut Ui, label: impl Into<String>) -> Response {
    let label = label.into();
    let r = ui.add(egui::Button::new(&label).min_size(vec2(0.0, 28.0)));
    track(&label, &r);
    r
}

/// A button for destructive actions.
pub fn danger_button(ui: &mut Ui, label: &str) -> Response {
    let p = palette(ui.ctx());
    let enabled = ui.is_enabled();
    let text = RichText::new(label).color(if enabled {
        Color32::WHITE
    } else {
        p.text_faint
    });
    let mut b = egui::Button::new(text).min_size(BUTTON_SIZE);
    if enabled {
        b = b.fill(if p.dark {
            Color32::from_rgb(0xB8, 0x3A, 0x31)
        } else {
            p.danger
        });
    }
    let r = ui.add(b);
    track(label, &r);
    r
}

/// A small frameless text button for secondary actions inside lists
/// ("Edit", "Remove").
pub fn link_button(ui: &mut Ui, label: &str) -> Response {
    let p = palette(ui.ctx());
    let r = ui.add(egui::Button::new(RichText::new(label).color(p.accent)).frame(false));
    track(label, &r);
    r
}

/// A menu entry with an optional icon and a right-aligned shortcut.
pub fn menu_item(
    ui: &mut Ui,
    glyph: Option<&str>,
    label: &str,
    shortcut: Option<&str>,
) -> Response {
    let p = palette(ui.ctx());
    let text = match glyph {
        Some(g) => format!("{g}  {label}"),
        None => label.to_string(),
    };
    let mut b = egui::Button::new(text).min_size(vec2(220.0, 0.0));
    if let Some(s) = shortcut {
        b = b.shortcut_text(RichText::new(s).color(p.text_faint));
    }
    let r = ui.add(b);
    track(label, &r);
    r
}

/// Segmented control: one of several mutually exclusive options.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str)]) -> bool {
    let p = palette(ui.ctx());
    let mut changed = false;
    Frame::new()
        .fill(p.hover)
        .corner_radius(CornerRadius::same(radius::CONTROL))
        .inner_margin(Margin::same(2))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.horizontal(|ui| {
                for (v, label) in options {
                    let selected = *value == *v;
                    let text =
                        RichText::new(*label).color(if selected { p.text } else { p.text_muted });
                    let b = egui::Button::new(text)
                        .min_size(vec2(0.0, 22.0))
                        .fill(if selected {
                            p.surface
                        } else {
                            Color32::TRANSPARENT
                        })
                        .stroke(if selected {
                            Stroke::new(1.0_f32, p.border)
                        } else {
                            Stroke::NONE
                        });
                    let r = ui.add(b);
                    track(label, &r);
                    if r.clicked() && !selected {
                        *value = *v;
                        changed = true;
                    }
                }
            });
        });
    changed
}

/// Tabs along the top of a panel. Returns true when the selection changed.
pub fn panel_tabs<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str)]) -> bool {
    let p = palette(ui.ctx());
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::L;
        for (v, label) in options {
            let selected = *value == *v;
            let text = RichText::new(*label)
                .family(theme::strong_family())
                .size(size::BODY)
                .color(if selected { p.text } else { p.text_muted });
            let r = ui.add(egui::Label::new(text).sense(Sense::click()));
            if selected {
                let y = r.rect.bottom() + 3.0;
                ui.painter()
                    .hline(r.rect.x_range(), y, Stroke::new(2.0_f32, p.accent));
            } else if r.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            track(label, &r);
            if r.clicked() && !selected {
                *value = *v;
                changed = true;
            }
        }
    });
    ui.add_space(space::S);
    changed
}

/// A small uppercase label that starts a section of a panel, with room for
/// controls on the right.
pub fn section_header(ui: &mut Ui, title: &str, right: impl FnOnce(&mut Ui)) {
    let p = palette(ui.ctx());
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(title.to_uppercase())
                .text_style(theme::section_style())
                .color(p.text_muted),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), right);
    });
}

/// Section label without controls.
pub fn section(ui: &mut Ui, title: &str) {
    section_header(ui, title, |_| {});
}

/// Secondary text.
pub fn muted(ui: &mut Ui, text: impl Into<String>) -> Response {
    let p = palette(ui.ctx());
    ui.label(RichText::new(text.into()).color(p.text_muted))
}

/// A horizontal rule with even space around it.
pub fn divider(ui: &mut Ui) {
    let p = palette(ui.ctx());
    ui.add_space(space::S);
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(w, 1.0), Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        Stroke::new(1.0_f32, p.border),
    );
    ui.add_space(space::S);
}

/// Tones for badges and notices.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Accent,
    Success,
    Warning,
    Danger,
}

impl Tone {
    pub fn colors(self, p: &Palette) -> (Color32, Color32) {
        match self {
            Tone::Neutral => (p.text_muted, p.hover),
            Tone::Accent => (p.accent, p.accent_soft),
            Tone::Success => (p.success, p.success_soft),
            Tone::Warning => (p.warning, p.warning_soft),
            Tone::Danger => (p.danger, p.danger_soft),
        }
    }
}

/// A small rounded label ("3 errors", "Tracking").
pub fn badge(ui: &mut Ui, text: impl Into<String>, tone: Tone) -> Response {
    let p = palette(ui.ctx());
    let (fg, bg) = tone.colors(&p);
    Frame::new()
        .fill(bg)
        .corner_radius(CornerRadius::same(9))
        .inner_margin(Margin::symmetric(7, 1))
        .show(ui, |ui| {
            ui.label(RichText::new(text.into()).size(size::SMALL).color(fg))
        })
        .inner
}

/// A coloured message box for a dialog or panel.
pub fn notice(ui: &mut Ui, tone: Tone, text: impl Into<String>) {
    let p = palette(ui.ctx());
    let (fg, bg) = tone.colors(&p);
    let glyph = match tone {
        Tone::Danger => icon::WARNING_CIRCLE,
        Tone::Warning => icon::WARNING,
        Tone::Success => icon::CHECK_CIRCLE,
        _ => icon::INFO,
    };
    Frame::new()
        .fill(bg)
        .corner_radius(CornerRadius::same(radius::CONTROL))
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(glyph).color(fg).size(size::ICON));
                ui.label(RichText::new(text.into()).color(if p.dark { p.text } else { fg }));
            });
        });
}

/// A keyboard key, for the shortcuts list.
pub fn kbd(ui: &mut Ui, keys: &str) {
    let p = palette(ui.ctx());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        for (i, part) in keys.split(" / ").enumerate() {
            if i > 0 {
                ui.label(RichText::new("/").color(p.text_faint));
            }
            Frame::new()
                .fill(p.surface)
                .stroke(Stroke::new(1.0_f32, p.border_strong))
                .corner_radius(CornerRadius::same(4))
                .inner_margin(Margin::symmetric(6, 1))
                .show(ui, |ui| {
                    ui.label(RichText::new(part).monospace().size(12.0).color(p.text));
                });
        }
    });
}

/// A centred placeholder for an empty panel or window.
pub fn empty_state(ui: &mut Ui, glyph: &str, title: &str, body: &str) {
    let p = palette(ui.ctx());
    ui.vertical_centered(|ui| {
        ui.add_space(space::XXL);
        ui.label(RichText::new(glyph).size(28.0).color(p.text_faint));
        ui.add_space(space::S);
        ui.label(RichText::new(title).color(p.text_muted).strong());
        if !body.is_empty() {
            ui.label(RichText::new(body).size(size::SMALL).color(p.text_faint));
        }
        ui.add_space(space::XL);
    });
}

/// Two-column form: muted labels on the left, controls on the right.
pub fn form(ui: &mut Ui, id: &str, rows: impl FnOnce(&mut Form<'_>)) {
    egui::Grid::new(id)
        .num_columns(2)
        .spacing(vec2(space::L, space::M))
        .min_col_width(96.0)
        .show(ui, |ui| rows(&mut Form { ui }));
}

pub struct Form<'a> {
    ui: &'a mut Ui,
}

impl Form<'_> {
    fn label(&mut self, label: &str) {
        let p = palette(self.ui.ctx());
        // A fixed label column, so separate forms in one dialog line up.
        self.ui.allocate_ui_with_layout(
            vec2(LABEL_WIDTH, 22.0),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.set_min_width(LABEL_WIDTH);
                ui.label(RichText::new(label).color(p.text_muted));
            },
        );
    }

    pub fn row<R>(&mut self, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
        self.label(label);
        let r = add(self.ui);
        self.ui.end_row();
        r
    }

    /// A row with a hint under the control.
    pub fn row_with_hint<R>(
        &mut self,
        label: &str,
        hint: &str,
        add: impl FnOnce(&mut Ui) -> R,
    ) -> R {
        let p = palette(self.ui.ctx());
        self.label(label);
        let r = self
            .ui
            .vertical(|ui| {
                let r = add(ui);
                ui.label(RichText::new(hint).size(size::SMALL).color(p.text_faint));
                r
            })
            .inner;
        self.ui.end_row();
        r
    }
}

/// A text field that registers itself for scripts under `label`. `width` is
/// the field's whole width, padding included, so a panel that hands out its
/// available width never grows.
pub fn text_field(ui: &mut Ui, label: &str, text: &mut String, hint: &str, width: f32) -> Response {
    let r = ui.add(
        egui::TextEdit::singleline(text)
            .hint_text(hint)
            .desired_width((width - 12.0).max(24.0))
            .margin(Margin::symmetric(6, 4)),
    );
    track(label, &r);
    r
}

/// What a dialog's footer reports.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogResult {
    /// The primary button was clicked, or Enter pressed.
    pub confirmed: bool,
    /// The dialog should close (cancel, close button or Escape).
    pub closed: bool,
}

/// A dialog window: title row with a close button, body, and an optional
/// footer of buttons aligned to the right. Escape closes it.
pub struct Dialog<'a> {
    id: egui::Id,
    title: &'a str,
    width: f32,
    modal: bool,
    resizable: bool,
}

impl<'a> Dialog<'a> {
    pub fn new(title: &'a str) -> Self {
        Self {
            id: egui::Id::new(("dialog", title)),
            title,
            width: 420.0,
            modal: true,
            resizable: false,
        }
    }

    pub fn id(mut self, id: impl std::hash::Hash) -> Self {
        self.id = egui::Id::new(("dialog", id));
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// Non-modal dialogs (Find, Find in files) leave the editor usable.
    pub fn modeless(mut self) -> Self {
        self.modal = false;
        self
    }

    /// Shows the dialog; `body` fills it. Returns whether it should close
    /// (Escape or the close button) alongside the body's result.
    pub fn show<R>(
        self,
        ctx: &egui::Context,
        body: impl FnOnce(&mut Ui) -> R,
    ) -> (bool, Option<R>) {
        let p = palette(ctx);
        let frame = Frame::new()
            .fill(p.raised)
            .stroke(Stroke::new(1.0_f32, p.border))
            .corner_radius(CornerRadius::same(radius::WINDOW))
            .shadow(ctx.style().visuals.window_shadow)
            .inner_margin(Margin::same(space::XL as i8));
        let mut close = false;
        let title = self.title;
        let width = self.width;
        let contents = |ui: &mut Ui, close: &mut bool| -> R {
            ui.set_width(width);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(title)
                        .text_style(theme::title_style())
                        .color(p.text),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if close_button(ui).clicked() {
                        *close = true;
                    }
                });
            });
            ui.add_space(space::M);
            body(ui)
        };
        let result = if self.modal {
            let modal = egui::Modal::new(self.id)
                .frame(frame)
                .backdrop_color(Color32::from_black_alpha(if p.dark { 110 } else { 70 }))
                .show(ctx, |ui| contents(ui, &mut close));
            if modal.should_close() {
                close = true;
            }
            Some(modal.inner)
        } else {
            let r = egui::Window::new(self.title)
                .id(self.id)
                .title_bar(false)
                .frame(frame)
                .resizable(self.resizable)
                .default_pos(egui::pos2(
                    ctx.content_rect().center().x - width / 2.0 - space::XL,
                    ctx.content_rect().top() + 90.0,
                ))
                .show(ctx, |ui| contents(ui, &mut close));
            let focused_here = r.as_ref().is_some_and(|r| {
                ctx.memory(|m| m.focused().is_some()) || r.response.contains_pointer()
            });
            if focused_here && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
            r.and_then(|r| r.inner)
        };
        (close, result)
    }
}

fn close_button(ui: &mut Ui) -> Response {
    let p = palette(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(radius::CONTROL), p.hover);
    }
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        icon::X,
        FontId::proportional(size::ICON),
        if resp.hovered() { p.text } else { p.text_muted },
    );
    let resp = resp.on_hover_text("Close (Esc)");
    track("Close", &resp);
    resp
}

/// The row of buttons at the bottom of a dialog, right-aligned in the
/// platform's usual order: the primary action last on the right, Cancel to
/// its left. Enter triggers the primary action unless a multi-line text
/// field has the keyboard.
pub fn dialog_footer(
    ui: &mut Ui,
    primary: Option<(&str, bool)>,
    cancel: Option<&str>,
    extra: impl FnOnce(&mut Ui),
) -> DialogResult {
    let mut res = DialogResult::default();
    ui.add_space(space::L);
    ui.horizontal(|ui| {
        extra(ui);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if let Some((label, enabled)) = primary {
                let r = ui
                    .add_enabled_ui(enabled, |ui| primary_button(ui, label))
                    .inner;
                if r.clicked() {
                    res.confirmed = true;
                }
                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);
                if enter && enabled && !multiline_focused(ui.ctx()) {
                    res.confirmed = true;
                }
            }
            if let Some(label) = cancel
                && button(ui, label).clicked()
            {
                res.closed = true;
            }
        });
    });
    res
}

/// True when the focused widget is a multi-line text editor (where Enter
/// means a new line, not "OK").
fn multiline_focused(ctx: &egui::Context) -> bool {
    ctx.memory(|m| m.focused())
        .is_some_and(|id| ctx.data(|d| d.get_temp::<bool>(id.with("multiline")).unwrap_or(false)))
}

/// Marks a text editor as multi-line for [`dialog_footer`]'s Enter handling.
pub fn mark_multiline(ui: &Ui, r: &Response) {
    ui.ctx()
        .data_mut(|d| d.insert_temp(r.id.with("multiline"), true));
}

/// Card used for list entries (review items, search results).
pub fn card<R>(
    ui: &mut Ui,
    selected: bool,
    add: impl FnOnce(&mut Ui) -> R,
) -> egui::InnerResponse<R> {
    let p = palette(ui.ctx());
    Frame::new()
        .fill(if selected { p.accent_soft } else { p.surface })
        .stroke(Stroke::new(
            1.0_f32,
            if selected { p.accent } else { p.border },
        ))
        .corner_radius(CornerRadius::same(radius::CARD))
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
}

/// Paints a rounded hover/selection background behind a row.
pub fn row_background(ui: &Ui, rect: Rect, selected: bool, hovered: bool) {
    let p = palette(ui.ctx());
    let fill = if selected {
        p.accent_soft
    } else if hovered {
        p.hover
    } else {
        return;
    };
    ui.painter().rect_filled(rect, CornerRadius::same(4), fill);
}

/// Text with an icon in front, in one colour.
pub fn icon_text(glyph: &str, text: &str) -> WidgetText {
    format!("{glyph}  {text}").into()
}

/// Width reserved for a label column in forms; kept here so dialogs align.
pub const LABEL_WIDTH: f32 = 120.0;

/// Size of a standard dialog button.
pub const BUTTON_SIZE: Vec2 = Vec2::new(84.0, 28.0);

/// One row of a tree or list panel.
#[derive(Default)]
pub struct Row<'a> {
    /// Left indent (tree depth × step).
    pub indent: f32,
    /// `Some(open)` draws a disclosure chevron.
    pub chevron: Option<bool>,
    /// Leaves room for a chevron without drawing one (tree leaves).
    pub chevron_space: bool,
    pub icon: Option<(&'a str, Color32)>,
    pub text: &'a str,
    pub text_color: Option<Color32>,
    pub monospace: bool,
    /// Secondary text after the main text, truncated to fit.
    pub detail: Option<&'a str>,
    pub selected: bool,
}

pub struct RowResponse {
    pub response: Response,
    /// The chevron was clicked.
    pub toggled: bool,
}

pub const ROW_HEIGHT: f32 = 24.0;

/// Draws a compact, full-width list row and returns its response.
pub fn list_row(ui: &mut Ui, row: Row<'_>) -> RowResponse {
    let p = palette(ui.ctx());
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, ROW_HEIGHT), Sense::click());
    let mut toggled = false;
    if !ui.is_rect_visible(rect) {
        return RowResponse { response, toggled };
    }
    row_background(ui, rect, row.selected, response.hovered());
    let painter = ui.painter_at(rect);
    let mut x = rect.left() + 4.0 + row.indent;
    let cy = rect.center().y;
    if let Some(open) = row.chevron {
        let hit = Rect::from_center_size(egui::pos2(x + 7.0, cy), vec2(18.0, ROW_HEIGHT));
        let hover = ui
            .ctx()
            .pointer_hover_pos()
            .is_some_and(|pos| hit.contains(pos));
        painter.text(
            egui::pos2(x + 7.0, cy),
            Align2::CENTER_CENTER,
            if open {
                icon::CARET_DOWN
            } else {
                icon::CARET_RIGHT
            },
            FontId::proportional(12.0),
            if hover { p.text } else { p.text_faint },
        );
        if response.clicked() && hover {
            toggled = true;
        }
        x += 16.0;
    } else if row.chevron_space {
        x += 16.0;
    }
    if let Some((glyph, color)) = row.icon {
        painter.text(
            egui::pos2(x + 7.0, cy),
            Align2::CENTER_CENTER,
            glyph,
            FontId::proportional(size::ICON - 1.0),
            color,
        );
        x += 20.0;
    }
    let font = if row.monospace {
        FontId::monospace(size::MONO - 0.5)
    } else {
        FontId::proportional(size::BODY)
    };
    let color = row.text_color.unwrap_or(p.text);
    let galley = painter.layout_no_wrap(row.text.to_string(), font, color);
    let text_w = galley.size().x;
    painter.galley(egui::pos2(x, cy - galley.size().y / 2.0), galley, color);
    x += text_w + 8.0;
    if let Some(detail) = row.detail.filter(|d| !d.is_empty()) {
        let room = rect.right() - x - 6.0;
        if room > 24.0 {
            let mut job = egui::text::LayoutJob::single_section(
                detail.to_string(),
                egui::TextFormat::simple(FontId::proportional(size::SMALL), p.text_faint),
            );
            job.wrap = egui::text::TextWrapping::truncate_at_width(room);
            let g = painter.layout_job(job);
            painter.galley(egui::pos2(x, cy - g.size().y / 2.0), g, p.text_faint);
        }
    }
    RowResponse { response, toggled }
}
