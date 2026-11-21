//! Side and bottom panels: structure tree, element list, attribute inspector
//! and the issues list.

use crate::doc::{DocState, Severity};
use crate::ui::{Row, Tone, icon, list_row, palette};
use egui::{RichText, Ui};
use xylem_dom::{NodeId, NodeKind};
use xylem_edit::Rules;
use xylem_edit::pos::{container, first_editable};

#[derive(Debug, Clone)]
pub enum PanelAction {
    Select(NodeId),
    InsertElement(String),
    Surround(String),
    Rename(NodeId, String),
    SetAttr(NodeId, String, String),
    RemoveAttr(NodeId, String),
    GotoIssue(Option<NodeId>),
    RemoveTags(NodeId),
    DeleteElement(NodeId),
    MoveElement(NodeId, bool),
    /// Collapse (true) or expand (false) an element in the document view.
    Collapse(NodeId, bool),
}

#[derive(Copy, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum ElementMode {
    #[default]
    Insert,
    Surround,
    Change,
}

#[derive(Default)]
pub struct PanelState {
    pub element_filter: String,
    pub element_mode: ElementMode,
    pub show_all_elements: bool,
    pub new_attr_name: String,
    pub new_attr_value: String,
    pub structure_expanded: std::collections::HashSet<NodeId>,
    pub structure_follow: Option<NodeId>,
}

/// The element the panels operate on: a selected element, else the caret container.
pub fn current_element(ds: &DocState) -> Option<NodeId> {
    let doc = &ds.editor.doc;
    let sel = ds.editor.sel;
    if !sel.is_caret() {
        let (a, b) = sel.ordered(doc);
        if a.node == b.node && doc.is_element(a.node) && b.offset == a.offset + 1 {
            let n = doc.children(a.node)[a.offset];
            if doc.is_element(n) {
                return Some(n);
            }
        }
    }
    let c = container(doc, sel.focus);
    if doc.is_element(c) {
        Some(c)
    } else {
        doc.document_element()
    }
}

// ------------------------------------------------------------------ structure

pub fn structure(ui: &mut Ui, ds: &DocState, st: &mut PanelState, out: &mut Vec<PanelAction>) {
    let doc = &ds.editor.doc;
    let Some(root) = doc.document_element() else {
        return;
    };
    let cur = current_element(ds);
    // Expand the path to the current element when it changes.
    if st.structure_follow != cur {
        st.structure_follow = cur;
        if let Some(c) = cur {
            for a in doc.ancestors(c) {
                st.structure_expanded.insert(a);
            }
        }
    }
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .id_salt("structure")
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            structure_node(ui, ds, root, cur, st, out, 0);
        });
}

fn structure_node(
    ui: &mut Ui,
    ds: &DocState,
    n: NodeId,
    cur: Option<NodeId>,
    st: &mut PanelState,
    out: &mut Vec<PanelAction>,
    depth: usize,
) {
    let p = palette(ui.ctx());
    let doc = &ds.editor.doc;
    let kids: Vec<NodeId> = doc.element_children(n).collect();
    let name = doc.name(n).unwrap_or("?");
    let flagged = ds.canvas.flagged.contains(&n);
    let is_cur = cur == Some(n);
    // A short preview of text content.
    let preview: String = doc
        .children(n)
        .iter()
        .filter_map(|&c| doc.text(c))
        .collect::<String>()
        .split_whitespace()
        .take(12)
        .collect::<Vec<_>>()
        .join(" ");
    let open = st.structure_expanded.contains(&n);
    let row = list_row(
        ui,
        Row {
            indent: depth as f32 * 14.0,
            chevron: (!kids.is_empty()).then_some(open),
            chevron_space: kids.is_empty(),
            text: name,
            text_color: Some(if flagged { p.danger } else { p.text }),
            detail: Some(&preview),
            selected: is_cur,
            ..Default::default()
        },
    );
    if row.toggled {
        if open {
            st.structure_expanded.remove(&n);
        } else {
            st.structure_expanded.insert(n);
        }
    } else if row.response.clicked() {
        out.push(PanelAction::Select(n));
    }
    if row.response.double_clicked() && !kids.is_empty() {
        if open {
            st.structure_expanded.remove(&n);
        } else {
            st.structure_expanded.insert(n);
        }
    }
    let resp = if flagged {
        row.response
            .on_hover_text("This element has validation problems")
    } else {
        row.response
    };
    resp.context_menu(|ui| {
        let collapsed = ds.canvas.collapsed.contains(&n);
        let items: [(&str, PanelAction); 5] = [
            ("Move up", PanelAction::MoveElement(n, true)),
            ("Move down", PanelAction::MoveElement(n, false)),
            (
                if collapsed {
                    "Expand in document"
                } else {
                    "Collapse in document"
                },
                PanelAction::Collapse(n, !collapsed),
            ),
            ("Remove tags", PanelAction::RemoveTags(n)),
            ("Delete element", PanelAction::DeleteElement(n)),
        ];
        for (i, (label, action)) in items.into_iter().enumerate() {
            if i == 2 || i == 3 {
                ui.separator();
            }
            if crate::ui::menu_item(ui, None, label, None).clicked() {
                out.push(action);
                ui.close();
            }
        }
    });
    if open {
        for k in kids {
            structure_node(ui, ds, k, cur, st, out, depth + 1);
        }
    }
}

// ------------------------------------------------------------------ elements

pub fn elements(
    ui: &mut Ui,
    ds: &DocState,
    rules: &dyn Rules,
    st: &mut PanelState,
    out: &mut Vec<PanelAction>,
) {
    let p = palette(ui.ctx());
    let doc = &ds.editor.doc;
    crate::ui::segmented(
        ui,
        &mut st.element_mode,
        &[
            (ElementMode::Insert, "Insert"),
            (ElementMode::Surround, "Surround"),
            (ElementMode::Change, "Change"),
        ],
    );
    ui.horizontal(|ui| {
        let w = (ui.available_width() - 56.0).max(80.0);
        crate::ui::text_field(ui, "Filter elements", &mut st.element_filter, "Filter…", w);
        ui.checkbox(&mut st.show_all_elements, "All")
            .on_hover_text("Also list elements that would make the document invalid here");
    });
    ui.add_space(2.0);
    let Some(g) = ds.grammar.as_deref() else {
        crate::ui::muted(ui, "No schema: type any element name in the filter box.");
        let mut name = st.element_filter.clone();
        if crate::ui::button(ui, "Insert element").clicked() && !name.trim().is_empty() {
            name = name.trim().to_string();
            out.push(match st.element_mode {
                ElementMode::Insert => PanelAction::InsertElement(name),
                ElementMode::Surround => PanelAction::Surround(name),
                ElementMode::Change => match current_element(ds) {
                    Some(e) => PanelAction::Rename(e, name),
                    None => return,
                },
            });
        }
        return;
    };
    let (parent, index) = ds.editor.insertion_point();
    let mut items: Vec<(String, bool)> = match st.element_mode {
        ElementMode::Insert => {
            let mut v: Vec<(String, bool)> = g
                .insertable_at(doc, parent, index)
                .into_iter()
                .map(|i| (i.name, i.keeps_valid))
                .collect();
            // Elements allowed after the current block (XMetaL "insert after").
            if v.is_empty() {
                let mut c = parent;
                while let Some(p) = doc.parent(c).filter(|&p| doc.is_element(p)) {
                    let idx = doc.index_in_parent(c).unwrap() + 1;
                    v.extend(
                        g.insertable_at(doc, p, idx)
                            .into_iter()
                            .map(|i| (i.name, i.keeps_valid)),
                    );
                    if !v.is_empty() {
                        break;
                    }
                    c = p;
                }
            }
            v
        }
        ElementMode::Surround => {
            let names = g.all_element_names();
            if ds.editor.sel.is_caret() {
                Vec::new()
            } else {
                let (a, _) = ds.editor.sel.ordered(doc);
                let p = container(doc, a);
                names
                    .into_iter()
                    .filter(|n| rules.can_insert(doc, p, 0, n) || st.show_all_elements)
                    .map(|n| (n, true))
                    .collect()
            }
        }
        ElementMode::Change => match current_element(ds) {
            Some(e) => g
                .valid_renames(doc, e)
                .into_iter()
                .map(|n| (n, true))
                .collect(),
            None => Vec::new(),
        },
    };
    if st.show_all_elements {
        let have: std::collections::HashSet<String> = items.iter().map(|i| i.0.clone()).collect();
        for n in g.all_element_names() {
            if !have.contains(&n) {
                items.push((n, false));
            }
        }
    }
    items.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then(a.0.to_lowercase().cmp(&b.0.to_lowercase()))
    });
    items.dedup_by(|a, b| a.0 == b.0);
    let filter = st.element_filter.to_lowercase();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .id_salt("elements")
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if items.is_empty() {
                crate::ui::empty_state(
                    ui,
                    icon::BRACKETS_ANGLE,
                    match st.element_mode {
                        ElementMode::Surround if ds.editor.sel.is_caret() => {
                            "Select some content to surround"
                        }
                        ElementMode::Change => "Nothing else is allowed here",
                        _ => "No elements are allowed here",
                    },
                    "",
                );
            }
            for (name, valid) in items
                .iter()
                .filter(|(n, _)| filter.is_empty() || n.to_lowercase().contains(&filter))
            {
                let row = list_row(
                    ui,
                    Row {
                        text: name,
                        text_color: Some(if *valid { p.text } else { p.text_faint }),
                        ..Default::default()
                    },
                );
                crate::script::track(&format!("element {name}"), &row.response);
                let desc = ds
                    .package
                    .descriptions
                    .get(name)
                    .cloned()
                    .or_else(|| g.documentation_for_name(name).map(str::to_string));
                let mut resp = row.response;
                if let Some(d) = desc.filter(|d| !d.is_empty()) {
                    resp = resp.on_hover_text(d);
                } else if !valid {
                    resp = resp.on_hover_text("Inserting this here makes the document invalid");
                }
                if resp.clicked() {
                    let a = match st.element_mode {
                        ElementMode::Insert => PanelAction::InsertElement(name.clone()),
                        ElementMode::Surround => PanelAction::Surround(name.clone()),
                        ElementMode::Change => match current_element(ds) {
                            Some(e) => PanelAction::Rename(e, name.clone()),
                            None => continue,
                        },
                    };
                    out.push(a);
                }
            }
        });
}

// ------------------------------------------------------------------ attributes

pub fn attributes(ui: &mut Ui, ds: &DocState, st: &mut PanelState, out: &mut Vec<PanelAction>) {
    let p = palette(ui.ctx());
    let doc = &ds.editor.doc;
    let Some(el) = current_element(ds) else {
        return;
    };
    let Some(e) = doc.element(el) else { return };
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("<{}>", e.name))
                .monospace()
                .color(p.accent),
        );
        if let Some(g) = ds.grammar.as_deref()
            && let Some(d) = g.element_decl_for(doc, el).and_then(|d| g.documentation(d))
        {
            ui.label(RichText::new(icon::INFO).color(p.text_faint))
                .on_hover_text(d);
        }
    });
    let decls = ds
        .grammar
        .as_deref()
        .and_then(|g| {
            g.element_decl_for(doc, el)
                .map(|d| g.attributes(d).to_vec())
        })
        .unwrap_or_default();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .id_salt("attrs")
        .show(ui, |ui| {
            let name_w = 96.0;
            egui::Grid::new(("attr-grid", el))
                .num_columns(3)
                .spacing([6.0, 4.0])
                .min_col_width(0.0)
                .show(ui, |ui| {
                    let mut shown = std::collections::HashSet::new();
                    for d in &decls {
                        shown.insert(d.name.clone());
                        let current = e.attr(&d.name).map(str::to_string);
                        let mut text = d.name.clone();
                        if d.required {
                            text.push_str(" *");
                        }
                        let color = if current.is_some() || d.required {
                            p.text_muted
                        } else {
                            p.text_faint
                        };
                        let lr = ui.add_sized(
                            [name_w, 22.0],
                            egui::Label::new(RichText::new(text).color(color)).truncate(),
                        );
                        let tip = match (&d.documentation, d.required) {
                            (Some(t), true) => format!("{} (required)\n{t}", d.name),
                            (Some(t), false) => format!("{}\n{t}", d.name),
                            (None, true) => format!("{} (required)", d.name),
                            (None, false) => d.name.clone(),
                        };
                        lr.on_hover_text(tip);
                        attr_editor(
                            ui,
                            el,
                            &d.name,
                            current.as_deref(),
                            d.default.as_deref(),
                            &d.enumeration(),
                            d.fixed.as_deref(),
                            out,
                        );
                        if current.is_some() && !d.required {
                            if remove_button(ui).clicked() {
                                out.push(PanelAction::RemoveAttr(el, d.name.clone()));
                            }
                        } else {
                            ui.label("");
                        }
                        ui.end_row();
                    }
                    for a in &e.attrs {
                        if shown.contains(&a.name) {
                            continue;
                        }
                        let undeclared = !decls.is_empty()
                            && !a.name.starts_with("xmlns")
                            && !a.name.starts_with("xsi:")
                            && !a.name.starts_with("xml:");
                        let lr = ui.add_sized(
                            [name_w, 22.0],
                            egui::Label::new(RichText::new(&a.name).color(if undeclared {
                                p.warning
                            } else {
                                p.text_muted
                            }))
                            .truncate(),
                        );
                        if undeclared {
                            lr.on_hover_text("Not declared in the schema");
                        } else {
                            lr.on_hover_text(&a.name);
                        }
                        attr_editor(ui, el, &a.name, Some(&a.value), None, &[], None, out);
                        if remove_button(ui).clicked() {
                            out.push(PanelAction::RemoveAttr(el, a.name.clone()));
                        }
                        ui.end_row();
                    }
                });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let w = ((ui.available_width() - 60.0) / 2.0).max(60.0);
                crate::ui::text_field(ui, "New attribute name", &mut st.new_attr_name, "name", w);
                crate::ui::text_field(
                    ui,
                    "New attribute value",
                    &mut st.new_attr_value,
                    "value",
                    w,
                );
                if crate::ui::button(ui, "Add").clicked() && !st.new_attr_name.trim().is_empty() {
                    out.push(PanelAction::SetAttr(
                        el,
                        st.new_attr_name.trim().to_string(),
                        st.new_attr_value.clone(),
                    ));
                    st.new_attr_name.clear();
                    st.new_attr_value.clear();
                }
            });
        });
}

fn remove_button(ui: &mut Ui) -> egui::Response {
    let p = palette(ui.ctx());
    let r = ui.add(
        egui::Button::new(RichText::new(icon::X).color(p.text_faint).size(12.0))
            .frame(false)
            .min_size(egui::vec2(20.0, 20.0)),
    );
    r.on_hover_text("Remove attribute")
}

#[allow(clippy::too_many_arguments)]
fn attr_editor(
    ui: &mut Ui,
    el: NodeId,
    name: &str,
    current: Option<&str>,
    default: Option<&str>,
    enums: &[String],
    fixed: Option<&str>,
    out: &mut Vec<PanelAction>,
) {
    let p = palette(ui.ctx());
    // Name column, editor, remove button: leave room for the button.
    let w = (ui.available_width() - 34.0).max(60.0);
    if let Some(f) = fixed {
        ui.label(RichText::new(f).color(p.text_faint))
            .on_hover_text("Fixed value");
        return;
    }
    if !enums.is_empty() {
        let shown = current.unwrap_or("");
        egui::ComboBox::from_id_salt((el, name))
            .selected_text(if shown.is_empty() {
                RichText::new(
                    default
                        .map(|d| format!("{d} (default)"))
                        .unwrap_or_default(),
                )
                .color(p.text_faint)
            } else {
                RichText::new(shown)
            })
            .width(w)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(
                        current.is_none(),
                        RichText::new("Not set").color(p.text_muted),
                    )
                    .clicked()
                {
                    out.push(PanelAction::RemoveAttr(el, name.to_string()));
                }
                for v in enums {
                    if ui
                        .selectable_label(current == Some(v.as_str()), v)
                        .clicked()
                    {
                        out.push(PanelAction::SetAttr(el, name.to_string(), v.clone()));
                    }
                }
            });
        return;
    }
    let id = ui.make_persistent_id(("attr-edit", el, name));
    let mut buf: String = ui
        .data_mut(|d| d.get_temp::<String>(id))
        .unwrap_or_else(|| current.unwrap_or("").to_string());
    let resp = ui.add(
        egui::TextEdit::singleline(&mut buf)
            .desired_width(w - 12.0)
            .margin(egui::Margin::symmetric(6, 3))
            .hint_text(default.unwrap_or("")),
    );
    crate::script::track(&format!("attribute {name}"), &resp);
    if resp.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, buf.clone()));
    } else {
        ui.data_mut(|d| d.remove::<String>(id));
    }
    if resp.lost_focus() && Some(buf.as_str()) != current && !(current.is_none() && buf.is_empty())
    {
        out.push(PanelAction::SetAttr(el, name.to_string(), buf));
        ui.data_mut(|d| d.remove::<String>(id));
    }
}

// ------------------------------------------------------------------ issues

pub fn issues(ui: &mut Ui, ds: &DocState, out: &mut Vec<PanelAction>) {
    let p = palette(ui.ctx());
    let doc = &ds.editor.doc;
    if ds.issues.is_empty() {
        ui.horizontal(|ui| {
            ui.label(RichText::new(icon::CHECK_CIRCLE).color(p.success));
            crate::ui::muted(
                ui,
                if ds.grammar.is_some() {
                    "The document is valid."
                } else {
                    "The document is well-formed. There is no schema to validate against."
                },
            );
        });
        return;
    }
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .id_salt("issues")
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (i, issue) in ds.issues.iter().enumerate() {
                let (glyph, tone) = match issue.severity {
                    Severity::Error => (icon::WARNING_CIRCLE, Tone::Danger),
                    Severity::Warning => (icon::WARNING, Tone::Warning),
                    Severity::Info => (icon::INFO, Tone::Accent),
                };
                let (color, _) = tone.colors(&p);
                let place = issue
                    .node
                    .filter(|n| doc.contains(*n))
                    .map(|n| {
                        if doc.is_element(n) {
                            doc.path(n)
                        } else {
                            doc.parent(n).map(|p| doc.path(p)).unwrap_or_default()
                        }
                    })
                    .unwrap_or_default();
                let detail = format!("{}  ·  {place}", issue.source);
                let row = list_row(
                    ui,
                    Row {
                        icon: Some((glyph, color)),
                        text: &issue.message,
                        detail: Some(&detail),
                        ..Default::default()
                    },
                );
                if row
                    .response
                    .on_hover_text(format!("{}\n{}  {place}", issue.message, issue.source))
                    .clicked()
                {
                    out.push(PanelAction::GotoIssue(issue.node));
                }
                if i > 2000 {
                    crate::ui::muted(ui, format!("… and {} more", ds.issues.len() - i));
                    break;
                }
            }
        });
}

/// Breadcrumb of the caret's ancestors (status bar).
pub fn breadcrumb(ui: &mut Ui, ds: &DocState, out: &mut Vec<PanelAction>) {
    let p = palette(ui.ctx());
    let doc = &ds.editor.doc;
    let c = container(doc, ds.editor.sel.focus);
    let mut chain: Vec<NodeId> = std::iter::once(c)
        .chain(doc.ancestors(c))
        .filter(|&n| doc.is_element(n))
        .collect();
    chain.reverse();
    let cur = current_element(ds);
    // Long paths keep their tail: the elements nearest the caret matter most.
    let skip = chain.len().saturating_sub(8);
    ui.spacing_mut().item_spacing.x = 2.0;
    if skip > 0 {
        ui.label(RichText::new("…").color(p.text_faint));
    }
    for (i, n) in chain.iter().enumerate().skip(skip) {
        if i > skip {
            ui.label(
                RichText::new(icon::CARET_RIGHT)
                    .size(10.0)
                    .color(p.text_faint),
            );
        }
        let name = doc.name(*n).unwrap_or("");
        let is_cur = cur == Some(*n);
        let text = RichText::new(name)
            .size(crate::ui::theme::size::SMALL)
            .color(if is_cur { p.accent } else { p.text_muted });
        let r = ui.add(
            egui::Button::new(text)
                .frame(false)
                .min_size(egui::vec2(0.0, 18.0)),
        );
        if r.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if r.on_hover_text("Select this element").clicked() {
            out.push(PanelAction::Select(*n));
        }
    }
    let _ = (first_editable, NodeKind::Comment);
}
