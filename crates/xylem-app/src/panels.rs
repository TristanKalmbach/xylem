//! Side and bottom panels: structure tree, element list, attribute inspector
//! and the issues list.

use crate::doc::{DocState, Severity};
use egui::{Color32, RichText, Ui};
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
    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .id_salt("structure")
        .show(ui, |ui| {
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
    let doc = &ds.editor.doc;
    let kids: Vec<NodeId> = doc.element_children(n).collect();
    let name = doc.name(n).unwrap_or("?");
    let flagged = ds.canvas.flagged.contains(&n);
    let is_cur = cur == Some(n);
    let mut label = RichText::new(name);
    if is_cur {
        label = label
            .strong()
            .background_color(ui.visuals().selection.bg_fill);
    }
    if flagged {
        label = label.color(Color32::from_rgb(220, 70, 70));
    }
    // A short preview of text content.
    let preview: String = doc
        .children(n)
        .iter()
        .filter_map(|&c| doc.text(c))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let preview = if preview.chars().count() > 40 {
        format!("{}…", preview.chars().take(40).collect::<String>())
    } else {
        preview
    };
    ui.horizontal(|ui| {
        ui.add_space(depth as f32 * 12.0);
        if kids.is_empty() {
            ui.add_space(16.0);
        } else {
            let open = st.structure_expanded.contains(&n);
            if ui.small_button(if open { "-" } else { "+" }).clicked() {
                if open {
                    st.structure_expanded.remove(&n);
                } else {
                    st.structure_expanded.insert(n);
                }
            }
        }
        let r = ui.selectable_label(is_cur, label);
        if is_cur && st.structure_follow == cur {
            // keep visible
        }
        if r.clicked() {
            out.push(PanelAction::Select(n));
        }
        r.context_menu(|ui| {
            if ui.button("Remove tags").clicked() {
                out.push(PanelAction::RemoveTags(n));
                ui.close();
            }
            if ui.button("Delete element").clicked() {
                out.push(PanelAction::DeleteElement(n));
                ui.close();
            }
            if ui.button("Move up").clicked() {
                out.push(PanelAction::MoveElement(n, true));
                ui.close();
            }
            if ui.button("Move down").clicked() {
                out.push(PanelAction::MoveElement(n, false));
                ui.close();
            }
        });
        if !preview.is_empty() {
            ui.label(RichText::new(preview).weak().small());
        }
    });
    if st.structure_expanded.contains(&n) {
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
    let doc = &ds.editor.doc;
    ui.horizontal(|ui| {
        ui.selectable_value(&mut st.element_mode, ElementMode::Insert, "Insert");
        ui.selectable_value(&mut st.element_mode, ElementMode::Surround, "Surround");
        ui.selectable_value(&mut st.element_mode, ElementMode::Change, "Change");
    });
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut st.element_filter)
                .hint_text("Filter…")
                .desired_width(120.0),
        );
        ui.checkbox(&mut st.show_all_elements, "All")
            .on_hover_text("Show every element, not only the valid ones");
    });
    ui.separator();
    let Some(g) = ds.grammar.as_deref() else {
        ui.label(RichText::new("No schema loaded: any element name can be inserted.").weak());
        let mut name = st.element_filter.clone();
        if ui.button("Insert <element>").clicked() && !name.trim().is_empty() {
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
                let (a, b) = ds.editor.sel.ordered(doc);
                let p = container(doc, a);
                let _ = b;
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
            if items.is_empty() {
                ui.label(
                    RichText::new(match st.element_mode {
                        ElementMode::Surround if ds.editor.sel.is_caret() => {
                            "Select content to surround."
                        }
                        _ => "No elements are allowed here.",
                    })
                    .weak(),
                );
            }
            for (name, valid) in items
                .iter()
                .filter(|(n, _)| filter.is_empty() || n.to_lowercase().contains(&filter))
            {
                let mut text = RichText::new(name);
                if !valid {
                    text = text.weak();
                }
                let resp = ui.selectable_label(false, text);
                let desc = ds
                    .package
                    .descriptions
                    .get(name)
                    .cloned()
                    .or_else(|| g.documentation_for_name(name).map(str::to_string));
                let resp = match desc {
                    Some(d) if !d.is_empty() => resp.on_hover_text(d),
                    _ => resp,
                };
                if resp.double_clicked()
                    || (resp.clicked() && ui.input(|i| i.modifiers.is_none()) && false)
                    || resp.clicked()
                {
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
    let doc = &ds.editor.doc;
    let Some(el) = current_element(ds) else {
        return;
    };
    let Some(e) = doc.element(el) else { return };
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("<{}>", e.name)).strong());
        if let Some(g) = ds.grammar.as_deref()
            && let Some(d) = g.element_decl_for(doc, el).and_then(|d| g.documentation(d))
        {
            ui.label("(?)").on_hover_text(d);
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
            egui::Grid::new(("attr-grid", el))
                .num_columns(3)
                .striped(true)
                .spacing([6.0, 4.0])
                .show(ui, |ui| {
                    let mut shown = std::collections::HashSet::new();
                    for d in &decls {
                        shown.insert(d.name.clone());
                        let current = e.attr(&d.name).map(str::to_string);
                        let mut label = RichText::new(&d.name);
                        if d.required {
                            label = label.strong();
                        }
                        if current.is_none() {
                            label = label.weak();
                        }
                        let lr = ui.label(label);
                        if let Some(doc_text) = &d.documentation {
                            lr.on_hover_text(doc_text);
                        }
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
                            if ui
                                .small_button("x")
                                .on_hover_text("Remove attribute")
                                .clicked()
                            {
                                out.push(PanelAction::RemoveAttr(el, d.name.clone()));
                            }
                        } else {
                            ui.label(if d.required { "*" } else { "" });
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
                        let mut label = RichText::new(&a.name);
                        if undeclared {
                            label = label.color(Color32::from_rgb(220, 120, 40));
                        }
                        ui.label(label);
                        attr_editor(ui, el, &a.name, Some(&a.value), None, &[], None, out);
                        if ui.small_button("x").clicked() {
                            out.push(PanelAction::RemoveAttr(el, a.name.clone()));
                        }
                        ui.end_row();
                    }
                });
            ui.separator();
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut st.new_attr_name)
                        .hint_text("name")
                        .desired_width(90.0),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut st.new_attr_value)
                        .hint_text("value")
                        .desired_width(110.0),
                );
                if ui.button("Add").clicked() && !st.new_attr_name.trim().is_empty() {
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
    if let Some(f) = fixed {
        ui.label(RichText::new(f).weak())
            .on_hover_text("Fixed value");
        return;
    }
    if !enums.is_empty() {
        let shown = current.unwrap_or("");
        egui::ComboBox::from_id_salt((el, name))
            .selected_text(if shown.is_empty() {
                default.map(|d| format!("({d})")).unwrap_or_default()
            } else {
                shown.to_string()
            })
            .width(150.0)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(current.is_none(), "(not set)")
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
    let w = (ui.available_width() - 30.0).max(80.0);
    let resp = ui.add(
        egui::TextEdit::singleline(&mut buf)
            .desired_width(w)
            .hint_text(default.unwrap_or("")),
    );
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
    let doc = &ds.editor.doc;
    if ds.issues.is_empty() {
        ui.label(
            RichText::new(if ds.grammar.is_some() {
                "✔ Document is valid."
            } else {
                "✔ Well-formed. (No schema to validate against.)"
            })
            .color(Color32::from_rgb(60, 160, 80)),
        );
        return;
    }
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .id_salt("issues")
        .show(ui, |ui| {
            for (i, issue) in ds.issues.iter().enumerate() {
                let (icon, color) = match issue.severity {
                    Severity::Error => ("⛔", Color32::from_rgb(220, 70, 70)),
                    Severity::Warning => ("⚠", Color32::from_rgb(220, 160, 40)),
                    Severity::Info => ("ℹ", Color32::from_rgb(90, 140, 220)),
                };
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
                let text = RichText::new(format!(
                    "{icon} [{}] {}   {place}",
                    issue.source, issue.message
                ))
                .color(color);
                if ui.selectable_label(false, text).clicked() {
                    out.push(PanelAction::GotoIssue(issue.node));
                }
                if i > 2000 {
                    ui.label(format!("… {} more", ds.issues.len() - i));
                    break;
                }
            }
        });
}

/// Breadcrumb of the caret's ancestors (status bar).
pub fn breadcrumb(ui: &mut Ui, ds: &DocState, out: &mut Vec<PanelAction>) {
    let doc = &ds.editor.doc;
    let c = container(doc, ds.editor.sel.focus);
    let mut chain: Vec<NodeId> = std::iter::once(c)
        .chain(doc.ancestors(c))
        .filter(|&n| doc.is_element(n))
        .collect();
    chain.reverse();
    let cur = current_element(ds);
    for (i, n) in chain.iter().enumerate() {
        if i > 0 {
            ui.label(RichText::new("›").weak());
        }
        let name = doc.name(*n).unwrap_or("");
        if ui.selectable_label(cur == Some(*n), name).clicked() {
            out.push(PanelAction::Select(*n));
        }
    }
    if let NodeKind::Text(_) = doc.kind(ds.editor.sel.focus.node) {
    } else if !ds.editor.sel.is_caret() {
    }
    let _ = first_editable;
}
