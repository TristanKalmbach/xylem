//! Table editing for CALS (`table/tgroup/tbody/row/entry`) and HTML
//! (`table/tbody/tr/td`) markup, mirroring the XMAX table commands.

use crate::pos::{Pos, first_editable};
use crate::{EditError, EditResult, Editor, Rules, Selection};
use xylem_dom::{Document, NodeId, local_name};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TableModel {
    Cals,
    Html,
}

/// The table context around a node.
#[derive(Clone, Debug)]
pub struct TableCtx {
    pub model: TableModel,
    pub table: NodeId,
    /// CALS `tgroup`, or the HTML table itself.
    pub group: NodeId,
    pub row: NodeId,
    pub cell: NodeId,
    /// Column index of the cell (0-based, span start).
    pub col: usize,
}

fn lname(doc: &Document, n: NodeId) -> &str {
    doc.name(n).map(local_name).unwrap_or("")
}

fn is_cell(doc: &Document, n: NodeId) -> Option<TableModel> {
    match lname(doc, n) {
        "entry" => Some(TableModel::Cals),
        "td" | "th" => Some(TableModel::Html),
        _ => None,
    }
}

fn is_row(name: &str) -> bool {
    matches!(name, "row" | "tr")
}

/// Finds the table context of a node, if it is inside a table cell.
pub fn table_ctx(doc: &Document, node: NodeId) -> Option<TableCtx> {
    let cell = std::iter::once(node)
        .chain(doc.ancestors(node))
        .find(|&n| is_cell(doc, n).is_some())?;
    let model = is_cell(doc, cell)?;
    let row = doc.parent(cell)?;
    if !is_row(lname(doc, row)) {
        return None;
    }
    let group = doc.ancestors(row).find(|&a| match model {
        TableModel::Cals => lname(doc, a) == "tgroup",
        TableModel::Html => lname(doc, a) == "table",
    })?;
    let table = match model {
        TableModel::Cals => doc
            .ancestors(group)
            .find(|&a| matches!(lname(doc, a), "table" | "informaltable"))
            .unwrap_or(group),
        TableModel::Html => group,
    };
    let col = cell_columns(doc, model, group, row)
        .into_iter()
        .find(|(c, _, _)| *c == cell)
        .map(|(_, s, _)| s)
        .unwrap_or(0);
    Some(TableCtx {
        model,
        table,
        group,
        row,
        cell,
        col,
    })
}

/// CALS column names in order from `colspec` elements.
fn colnames(doc: &Document, group: NodeId) -> Vec<String> {
    doc.element_children(group)
        .filter(|&c| lname(doc, c) == "colspec")
        .enumerate()
        .map(|(i, c)| {
            doc.attr(c, "colname")
                .map(str::to_string)
                .unwrap_or_else(|| format!("col{}", i + 1))
        })
        .collect()
}

/// Cells of a row with their (start, end) column indices (inclusive).
pub fn cell_columns(
    doc: &Document,
    model: TableModel,
    group: NodeId,
    row: NodeId,
) -> Vec<(NodeId, usize, usize)> {
    let names = if model == TableModel::Cals {
        colnames(doc, group)
    } else {
        Vec::new()
    };
    let idx = |n: &str| names.iter().position(|c| c == n);
    let mut next = 0;
    let mut out = Vec::new();
    for c in doc.element_children(row).collect::<Vec<_>>() {
        if is_cell(doc, c).is_none() {
            continue;
        }
        let (s, e) = match model {
            TableModel::Cals => {
                let s = doc
                    .attr(c, "namest")
                    .and_then(idx)
                    .or_else(|| doc.attr(c, "colname").and_then(idx))
                    .unwrap_or(next);
                let e = doc.attr(c, "nameend").and_then(idx).unwrap_or(s);
                (s, e.max(s))
            }
            TableModel::Html => {
                let span = doc
                    .attr(c, "colspan")
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(1)
                    .max(1);
                (next, next + span - 1)
            }
        };
        next = e + 1;
        out.push((c, s, e));
    }
    out
}

/// All rows of the table group in document order.
pub fn rows(doc: &Document, group: NodeId) -> Vec<NodeId> {
    doc.descendants(group)
        .filter(|&n| doc.is_element(n) && is_row(lname(doc, n)))
        .filter(|&n| {
            // Exclude rows of nested tables.
            doc.ancestors(n)
                .find(|&a| matches!(lname(doc, a), "tgroup" | "table"))
                == Some(group)
                || doc
                    .ancestors(n)
                    .find(|&a| lname(doc, a) == "tgroup")
                    .is_none()
                    && doc.ancestors(n).find(|&a| lname(doc, a) == "table") == Some(group)
        })
        .collect()
}

pub fn column_count(doc: &Document, ctx: &TableCtx) -> usize {
    match ctx.model {
        TableModel::Cals => doc
            .attr(ctx.group, "cols")
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| colnames(doc, ctx.group).len().max(1)),
        TableModel::Html => rows(doc, ctx.group)
            .iter()
            .map(|&r| {
                cell_columns(doc, ctx.model, ctx.group, r)
                    .last()
                    .map(|x| x.2 + 1)
                    .unwrap_or(0)
            })
            .max()
            .unwrap_or(1),
    }
}

fn cell_name(doc: &Document, ctx: &TableCtx) -> String {
    doc.name(ctx.cell).unwrap_or("entry").to_string()
}

/// XML for a new CALS table with `rows` x `cols` (plus optional header row).
pub fn cals_table_xml(rows: usize, cols: usize, header: bool, title: Option<&str>) -> String {
    let mut s = String::from("<table>");
    if let Some(t) = title {
        s.push_str(&format!("<title>{t}</title>"));
    }
    s.push_str(&format!("<tgroup cols=\"{cols}\">"));
    for c in 1..=cols {
        s.push_str(&format!("<colspec colname=\"col{c}\" colwidth=\"1*\"/>"));
    }
    let row = |s: &mut String| {
        s.push_str("<row>");
        for _ in 0..cols {
            s.push_str("<entry></entry>");
        }
        s.push_str("</row>");
    };
    if header {
        s.push_str("<thead>");
        row(&mut s);
        s.push_str("</thead>");
    }
    s.push_str("<tbody>");
    for _ in 0..rows.max(1) {
        row(&mut s);
    }
    s.push_str("</tbody></tgroup></table>");
    s
}

pub fn html_table_xml(rows: usize, cols: usize, header: bool) -> String {
    let mut s = String::from("<table>");
    if header {
        s.push_str("<thead><tr>");
        for _ in 0..cols {
            s.push_str("<th></th>");
        }
        s.push_str("</tr></thead>");
    }
    s.push_str("<tbody>");
    for _ in 0..rows.max(1) {
        s.push_str("<tr>");
        for _ in 0..cols {
            s.push_str("<td></td>");
        }
        s.push_str("</tr>");
    }
    s.push_str("</tbody></table>");
    s
}

impl Editor {
    fn table_here(&self) -> EditResult<TableCtx> {
        let c = crate::pos::container(&self.doc, self.caret());
        table_ctx(&self.doc, c)
            .ok_or_else(|| EditError::NotAllowed("The caret is not in a table cell.".into()))
    }

    pub fn in_table(&self) -> bool {
        self.table_here().is_ok()
    }

    /// Inserts `n` rows above or below the current row.
    pub fn table_insert_rows(&mut self, rules: &dyn Rules, n: usize, below: bool) -> EditResult {
        let ctx = self.table_here()?;
        self.transact(|ed| {
            let parent = ed.doc.parent(ctx.row).unwrap();
            let at = ed.doc.index_in_parent(ctx.row).unwrap() + usize::from(below);
            let row_name = ed.doc.name(ctx.row).unwrap().to_string();
            let cells = cell_columns(&ed.doc, ctx.model, ctx.group, ctx.row);
            let mut first_new = None;
            for k in 0..n.max(1) {
                let r = ed.doc.create_element(row_name.clone());
                for &(c, _, _) in &cells {
                    let cell = ed.doc.create_element(ed.doc.name(c).unwrap().to_string());
                    for a in ed.doc.element(c).unwrap().attrs.clone() {
                        if matches!(
                            a.name.as_str(),
                            "namest" | "nameend" | "colname" | "colspan" | "align" | "valign"
                        ) {
                            ed.doc.set_attr(cell, &a.name, a.value);
                        }
                    }
                    ed.doc.append_child(r, cell);
                }
                ed.doc.insert_child(parent, at + k, r);
                first_new.get_or_insert(r);
            }
            let first_cell = ed.doc.element_children(first_new.unwrap()).next().unwrap();
            ed.sel = Selection::caret(first_editable(&ed.doc, rules, first_cell));
            Ok(())
        })
    }

    pub fn table_delete_row(&mut self, rules: &dyn Rules) -> EditResult {
        let ctx = self.table_here()?;
        if rows(&self.doc, ctx.group).len() <= 1 {
            return self.table_delete(rules);
        }
        self.transact(|ed| {
            let next = ed
                .doc
                .next_sibling(ctx.row)
                .or(ed.doc.prev_sibling(ctx.row));
            ed.doc.detach(ctx.row);
            let target = next
                .filter(|&n| ed.doc.is_element(n))
                .or_else(|| rows(&ed.doc, ctx.group).first().copied())
                .unwrap();
            ed.sel = Selection::caret(first_editable(&ed.doc, rules, target));
            Ok(())
        })
    }

    pub fn table_move_row(&mut self, rules: &dyn Rules, up: bool) -> EditResult {
        let ctx = self.table_here()?;
        self.move_element(rules, ctx.row, up)
    }

    /// Inserts a column left or right of the current one.
    pub fn table_insert_column(&mut self, rules: &dyn Rules, right: bool) -> EditResult {
        let ctx = self.table_here()?;
        let at = ctx.col + usize::from(right);
        self.transact(|ed| {
            let cell_name = cell_name(&ed.doc, &ctx);
            // Where each row receives its new cell, computed on the old grid.
            let inserts: Vec<(NodeId, usize, String)> = rows(&ed.doc, ctx.group)
                .into_iter()
                .map(|r| {
                    let cells = cell_columns(&ed.doc, ctx.model, ctx.group, r);
                    let idx = cells
                        .iter()
                        .find(|&&(_, s, _)| s >= at)
                        .map(|c| ed.doc.index_in_parent(c.0).unwrap())
                        .unwrap_or(ed.doc.children(r).len());
                    let name = cells
                        .first()
                        .map(|c| ed.doc.name(c.0).unwrap().to_string())
                        .unwrap_or(cell_name.clone());
                    (r, idx, name)
                })
                .collect();
            if ctx.model == TableModel::Cals {
                let cols = column_count(&ed.doc, &ctx) + 1;
                ed.doc.set_attr(ctx.group, "cols", cols.to_string());
                // Rebuild colspecs with fresh names, remapping spans.
                let old = colnames(&ed.doc, ctx.group);
                let specs: Vec<NodeId> = ed
                    .doc
                    .element_children(ctx.group)
                    .filter(|&c| lname(&ed.doc, c) == "colspec")
                    .collect();
                let new_names: Vec<String> = (1..=cols).map(|i| format!("col{i}")).collect();
                let remap = |i: usize| if i >= at { i + 1 } else { i };
                for r in rows(&ed.doc, ctx.group) {
                    for (c, s, e) in cell_columns(&ed.doc, ctx.model, ctx.group, r) {
                        for (attr, v) in [("colname", s), ("namest", s), ("nameend", e)] {
                            if ed.doc.attr(c, attr).is_some() {
                                ed.doc.set_attr(c, attr, new_names[remap(v)].clone());
                            }
                        }
                    }
                }
                if !specs.is_empty() || !old.is_empty() {
                    let insert_idx = specs
                        .get(at)
                        .and_then(|&s| ed.doc.index_in_parent(s))
                        .unwrap_or_else(|| {
                            specs
                                .last()
                                .map(|&s| ed.doc.index_in_parent(s).unwrap() + 1)
                                .unwrap_or(0)
                        });
                    let spec = ed.doc.create_element(
                        ed.doc
                            .name(specs.first().copied().unwrap_or(ctx.group))
                            .filter(|n| local_name(n) == "colspec")
                            .unwrap_or("colspec")
                            .to_string(),
                    );
                    ed.doc.set_attr(spec, "colwidth", "1*");
                    ed.doc.insert_child(ctx.group, insert_idx, spec);
                    let specs: Vec<NodeId> = ed
                        .doc
                        .element_children(ctx.group)
                        .filter(|&c| lname(&ed.doc, c) == "colspec")
                        .collect();
                    for (i, s) in specs.iter().enumerate() {
                        ed.doc.set_attr(*s, "colname", new_names[i].clone());
                        ed.doc.set_attr(*s, "colnum", (i + 1).to_string());
                    }
                }
            }
            for (r, idx, name) in inserts {
                let cell = ed.doc.create_element(name);
                if ctx.model == TableModel::Cals
                    && ed
                        .doc
                        .element_children(r)
                        .any(|c| ed.doc.attr(c, "colname").is_some())
                {
                    ed.doc.set_attr(cell, "colname", format!("col{}", at + 1));
                }
                ed.doc.insert_child(r, idx, cell);
            }
            ed.fix_selection(rules);
            Ok(())
        })
    }

    pub fn table_delete_column(&mut self, rules: &dyn Rules) -> EditResult {
        let ctx = self.table_here()?;
        if column_count(&self.doc, &ctx) <= 1 {
            return self.table_delete(rules);
        }
        self.transact(|ed| {
            for r in rows(&ed.doc, ctx.group) {
                if let Some(&(c, _, _)) = cell_columns(&ed.doc, ctx.model, ctx.group, r)
                    .iter()
                    .find(|&&(_, s, e)| s <= ctx.col && ctx.col <= e)
                {
                    ed.doc.detach(c);
                }
            }
            if ctx.model == TableModel::Cals {
                let specs: Vec<NodeId> = ed
                    .doc
                    .element_children(ctx.group)
                    .filter(|&c| lname(&ed.doc, c) == "colspec")
                    .collect();
                if let Some(&s) = specs.get(ctx.col) {
                    ed.doc.detach(s);
                }
                let cols = column_count(&ed.doc, &ctx).saturating_sub(1).max(1);
                ed.doc.set_attr(ctx.group, "cols", cols.to_string());
            }
            let target = ed.doc.element_children(ctx.row).next().unwrap_or(ctx.row);
            ed.sel = Selection::caret(first_editable(&ed.doc, rules, target));
            Ok(())
        })
    }

    /// Merges the current cell with its right neighbour.
    pub fn table_merge_right(&mut self, rules: &dyn Rules) -> EditResult {
        let ctx = self.table_here()?;
        let cells = cell_columns(&self.doc, ctx.model, ctx.group, ctx.row);
        let i = cells.iter().position(|c| c.0 == ctx.cell).unwrap();
        let Some(&(right, _, re)) = cells.get(i + 1) else {
            return Err(EditError::Nothing);
        };
        let (_, s, _) = cells[i];
        self.transact(|ed| {
            let kids = ed.doc.children(right).to_vec();
            for k in kids {
                ed.doc.append_child(ctx.cell, k);
            }
            ed.doc.detach(right);
            match ctx.model {
                TableModel::Cals => {
                    let names = colnames(&ed.doc, ctx.group);
                    if names.len() > re {
                        ed.doc.remove_attr(ctx.cell, "colname");
                        ed.doc.set_attr(ctx.cell, "namest", names[s].clone());
                        ed.doc.set_attr(ctx.cell, "nameend", names[re].clone());
                    }
                }
                TableModel::Html => ed
                    .doc
                    .set_attr(ctx.cell, "colspan", (re - s + 1).to_string()),
            }
            ed.sel = Selection::caret(first_editable(&ed.doc, rules, ctx.cell));
            Ok(())
        })
    }

    /// Merges the current cell with the cell below (vertical span).
    pub fn table_merge_down(&mut self, rules: &dyn Rules) -> EditResult {
        let ctx = self.table_here()?;
        let all = rows(&self.doc, ctx.group);
        let ri = all.iter().position(|&r| r == ctx.row).unwrap();
        let span = match ctx.model {
            TableModel::Cals => self
                .doc
                .attr(ctx.cell, "morerows")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0),
            TableModel::Html => {
                self.doc
                    .attr(ctx.cell, "rowspan")
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(1)
                    - 1
            }
        };
        let Some(&below) = all.get(ri + span + 1) else {
            return Err(EditError::Nothing);
        };
        let target = cell_columns(&self.doc, ctx.model, ctx.group, below)
            .into_iter()
            .find(|&(_, s, _)| s == ctx.col);
        let Some((bcell, _, _)) = target else {
            return Err(EditError::Nothing);
        };
        self.transact(|ed| {
            for k in ed.doc.children(bcell).to_vec() {
                ed.doc.append_child(ctx.cell, k);
            }
            ed.doc.detach(bcell);
            match ctx.model {
                TableModel::Cals => ed
                    .doc
                    .set_attr(ctx.cell, "morerows", (span + 1).to_string()),
                TableModel::Html => ed.doc.set_attr(ctx.cell, "rowspan", (span + 2).to_string()),
            }
            ed.sel = Selection::caret(first_editable(&ed.doc, rules, ctx.cell));
            Ok(())
        })
    }

    /// Removes horizontal and vertical spans from the current cell, adding
    /// empty cells to restore the grid.
    pub fn table_split_cell(&mut self, rules: &dyn Rules) -> EditResult {
        let ctx = self.table_here()?;
        let (s, e) = cell_columns(&self.doc, ctx.model, ctx.group, ctx.row)
            .into_iter()
            .find(|c| c.0 == ctx.cell)
            .map(|c| (c.1, c.2))
            .unwrap();
        let vspan = match ctx.model {
            TableModel::Cals => self
                .doc
                .attr(ctx.cell, "morerows")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0),
            TableModel::Html => self
                .doc
                .attr(ctx.cell, "rowspan")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(1)
                .saturating_sub(1),
        };
        if s == e && vspan == 0 {
            return Err(EditError::Nothing);
        }
        self.transact(|ed| {
            let name = ed.doc.name(ctx.cell).unwrap().to_string();
            let names = colnames(&ed.doc, ctx.group);
            for a in ["namest", "nameend", "colspan", "morerows", "rowspan"] {
                ed.doc.remove_attr(ctx.cell, a);
            }
            if ctx.model == TableModel::Cals && !names.is_empty() {
                ed.doc.set_attr(ctx.cell, "colname", names[s].clone());
            }
            let idx = ed.doc.index_in_parent(ctx.cell).unwrap();
            for (k, col) in (s + 1..=e).enumerate() {
                let c = ed.doc.create_element(name.clone());
                if ctx.model == TableModel::Cals && names.len() > col {
                    ed.doc.set_attr(c, "colname", names[col].clone());
                }
                ed.doc.insert_child(ctx.row, idx + 1 + k, c);
            }
            let all = rows(&ed.doc, ctx.group);
            let ri = all.iter().position(|&r| r == ctx.row).unwrap();
            for r in all.iter().skip(ri + 1).take(vspan) {
                let cells = cell_columns(&ed.doc, ctx.model, ctx.group, *r);
                let at = cells
                    .iter()
                    .position(|&(_, cs, _)| cs > s)
                    .map(|p| ed.doc.index_in_parent(cells[p].0).unwrap())
                    .unwrap_or(ed.doc.children(*r).len());
                for (k, col) in (s..=e).enumerate() {
                    let c = ed.doc.create_element(name.clone());
                    if ctx.model == TableModel::Cals && names.len() > col {
                        ed.doc.set_attr(c, "colname", names[col].clone());
                    }
                    ed.doc.insert_child(*r, at + k, c);
                }
            }
            ed.sel = Selection::caret(first_editable(&ed.doc, rules, ctx.cell));
            Ok(())
        })
    }

    pub fn table_delete(&mut self, rules: &dyn Rules) -> EditResult {
        let ctx = self.table_here()?;
        self.transact(|ed| {
            let parent = ed.doc.parent(ctx.table).unwrap();
            let idx = ed.doc.index_in_parent(ctx.table).unwrap();
            ed.doc.detach(ctx.table);
            ed.sel = Selection::caret(crate::pos::canonical(&ed.doc, rules, Pos::new(parent, idx)));
            Ok(())
        })
    }

    /// Moves the caret to the next/previous cell (Tab / Shift+Tab).
    pub fn table_next_cell(&mut self, rules: &dyn Rules, forward: bool) -> bool {
        let Ok(ctx) = self.table_here() else {
            return false;
        };
        let cells: Vec<NodeId> = rows(&self.doc, ctx.group)
            .into_iter()
            .flat_map(|r| self.doc.element_children(r).collect::<Vec<_>>())
            .collect();
        let i = cells.iter().position(|&c| c == ctx.cell).unwrap();
        let j = if forward { i + 1 } else { i.wrapping_sub(1) };
        if let Some(&c) = cells.get(j) {
            self.set_caret(first_editable(&self.doc, rules, c));
            true
        } else if forward {
            let _ = self.table_insert_rows(rules, 1, true);
            true
        } else {
            false
        }
    }
}
