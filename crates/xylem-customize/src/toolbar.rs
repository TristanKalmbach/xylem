//! Toolbar entries and the editor commands they trigger.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::behaviour::Behaviour;
use crate::error::Error;

/// An editor command. Serialised as a string: `save`, `toggle:bold`,
/// `insert-element:para`, `surround:emphasis`, ...
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Insert the named element (using its template when one exists).
    InsertElement(String),
    /// Toggle an inline style kind (`bold`, `italic`, ...); resolved through
    /// `[behaviour.toggles]`.
    Toggle(String),
    /// Wrap the selection in the named element.
    Surround(String),
    Open,
    Save,
    SaveAs,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    Find,
    Replace,
    Validate,
    /// Visual gap between button groups; carries no command.
    Separator,
}

impl Action {
    fn parts(&self) -> (&'static str, Option<&str>) {
        match self {
            Self::InsertElement(e) => ("insert-element", Some(e)),
            Self::Toggle(k) => ("toggle", Some(k)),
            Self::Surround(e) => ("surround", Some(e)),
            Self::Open => ("open", None),
            Self::Save => ("save", None),
            Self::SaveAs => ("save-as", None),
            Self::Undo => ("undo", None),
            Self::Redo => ("redo", None),
            Self::Cut => ("cut", None),
            Self::Copy => ("copy", None),
            Self::Paste => ("paste", None),
            Self::Find => ("find", None),
            Self::Replace => ("replace", None),
            Self::Validate => ("validate", None),
            Self::Separator => ("separator", None),
        }
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.parts() {
            (name, Some(arg)) => write!(f, "{name}:{arg}"),
            (name, None) => f.write_str(name),
        }
    }
}

impl FromStr for Action {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        let bad = || Error::BadAction(s.to_string());
        if let Some((name, arg)) = s.split_once(':') {
            if arg.is_empty() {
                return Err(bad());
            }
            return match name {
                "insert-element" => Ok(Self::InsertElement(arg.into())),
                "toggle" => Ok(Self::Toggle(arg.into())),
                "surround" => Ok(Self::Surround(arg.into())),
                _ => Err(bad()),
            };
        }
        Ok(match s {
            "open" => Self::Open,
            "save" => Self::Save,
            "save-as" => Self::SaveAs,
            "undo" => Self::Undo,
            "redo" => Self::Redo,
            "cut" => Self::Cut,
            "copy" => Self::Copy,
            "paste" => Self::Paste,
            "find" => Self::Find,
            "replace" => Self::Replace,
            "validate" => Self::Validate,
            "separator" => Self::Separator,
            _ => return Err(bad()),
        })
    }
}

/// One `[[toolbar]]` entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolbarEntry {
    pub id: String,
    pub label: String,
    #[serde(with = "action_string")]
    pub action: Action,
    /// Element the button relates to (lets the UI show an active state).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub element: Option<String>,
    /// Icon name from the editor's icon set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Shortcut such as `ctrl+b`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shortcut: Option<String>,
}

mod action_string {
    use super::Action;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(a: &Action, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(a)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Action, D::Error> {
        let text = String::deserialize(d)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

impl ToolbarEntry {
    fn new(id: &str, label: &str, action: Action) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            action,
            element: None,
            icon: None,
            shortcut: None,
        }
    }

    fn icon(mut self, icon: &str) -> Self {
        self.icon = Some(icon.into());
        self
    }

    fn shortcut(mut self, shortcut: &str) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    fn element(mut self, element: &str) -> Self {
        self.element = Some(element.into());
        self
    }

    fn separator(n: usize) -> Self {
        Self::new(&format!("separator-{n}"), "", Action::Separator)
    }
}

/// Toggle kinds with a fixed order, label and shortcut; others follow
/// alphabetically.
const KNOWN_TOGGLES: &[(&str, &str, Option<&str>)] = &[
    ("bold", "Bold", Some("ctrl+b")),
    ("italic", "Italic", Some("ctrl+i")),
    ("underline", "Underline", Some("ctrl+u")),
    ("superscript", "Superscript", None),
    ("subscript", "Subscript", None),
];

/// Builds the toolbar used when a package does not define its own:
/// file/edit commands, then one button per toggle, paragraph, list and image
/// the behaviour declares.
pub fn default_toolbar(behaviour: &Behaviour) -> Vec<ToolbarEntry> {
    let mut bar = vec![
        ToolbarEntry::new("save", "Save", Action::Save)
            .icon("save")
            .shortcut("ctrl+s"),
        ToolbarEntry::new("undo", "Undo", Action::Undo)
            .icon("undo")
            .shortcut("ctrl+z"),
        ToolbarEntry::new("redo", "Redo", Action::Redo)
            .icon("redo")
            .shortcut("ctrl+y"),
        ToolbarEntry::new("validate", "Validate", Action::Validate)
            .icon("validate")
            .shortcut("f7"),
    ];

    let mut kinds: Vec<(&str, String, Option<&str>)> = KNOWN_TOGGLES
        .iter()
        .filter(|(kind, ..)| behaviour.toggles.contains_key(*kind))
        .map(|&(kind, label, key)| (kind, label.to_string(), key))
        .collect();
    for kind in behaviour.toggles.keys() {
        if !KNOWN_TOGGLES.iter().any(|(k, ..)| k == kind) {
            kinds.push((kind, capitalise(kind), None));
        }
    }
    let mut group = Vec::new();
    for (kind, label, key) in kinds {
        let mut entry = ToolbarEntry::new(kind, &label, Action::Toggle(kind.into()))
            .icon(kind)
            .element(&behaviour.toggles[kind]);
        if let Some(key) = key {
            entry = entry.shortcut(key);
        }
        group.push(entry);
    }
    push_group(&mut bar, group);

    let mut group = Vec::new();
    if let Some(p) = behaviour.paragraphs.first() {
        group.push(insert_button(p, "Paragraph", "paragraph"));
    }
    push_buttons(
        &mut group,
        behaviour.bulleted_lists.iter().map(|l| &l.name).collect(),
        "Bulleted list",
        "list-bulleted",
    );
    push_buttons(
        &mut group,
        behaviour.numbered_lists.iter().map(|l| &l.name).collect(),
        "Numbered list",
        "list-numbered",
    );
    push_buttons(
        &mut group,
        behaviour.definition_lists.iter().map(|l| &l.name).collect(),
        "Definition list",
        "list-definition",
    );
    push_buttons(
        &mut group,
        behaviour.images.iter().map(|i| &i.element).collect(),
        "Image",
        "image",
    );
    push_group(&mut bar, group);
    bar
}

fn push_group(bar: &mut Vec<ToolbarEntry>, group: Vec<ToolbarEntry>) {
    if group.is_empty() {
        return;
    }
    let n = bar.iter().filter(|e| e.action == Action::Separator).count() + 1;
    bar.push(ToolbarEntry::separator(n));
    bar.extend(group);
}

fn insert_button(element: &str, label: &str, icon: &str) -> ToolbarEntry {
    ToolbarEntry::new(
        &format!("insert-{element}"),
        label,
        Action::InsertElement(element.into()),
    )
    .icon(icon)
    .element(element)
}

/// One button per element; labels carry the element name when several share a kind.
fn push_buttons(group: &mut Vec<ToolbarEntry>, names: Vec<&String>, label: &str, icon: &str) {
    for name in &names {
        if group
            .iter()
            .any(|e| e.element.as_deref() == Some(name.as_str()))
        {
            continue;
        }
        let label = if names.len() > 1 {
            format!("{label}: {name}")
        } else {
            label.to_string()
        };
        group.push(insert_button(name, &label, icon));
    }
}

fn capitalise(s: &str) -> String {
    let mut chars = s.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::behaviour::ListDef;

    #[test]
    fn action_round_trip() {
        for text in [
            "save",
            "toggle:bold",
            "insert-element:para",
            "surround:emphasis",
            "separator",
        ] {
            let a: Action = text.parse().unwrap();
            assert_eq!(a.to_string(), text);
        }
        assert!("toggle:".parse::<Action>().is_err());
        assert!("explode".parse::<Action>().is_err());
        assert!("save:x".parse::<Action>().is_err());
    }

    #[test]
    fn default_toolbar_follows_behaviour() {
        let mut b = Behaviour::default();
        b.toggles.insert("italic".into(), "emphasis".into());
        b.toggles.insert("bold".into(), "strong".into());
        b.paragraphs.push("para".into());
        b.bulleted_lists.push(ListDef {
            name: "bulletedList".into(),
            header: None,
            item: "listItem".into(),
        });
        let bar = default_toolbar(&b);
        let actions: Vec<String> = bar.iter().map(|e| e.action.to_string()).collect();
        assert_eq!(&actions[..4], ["save", "undo", "redo", "validate"]);
        let bold = actions.iter().position(|a| a == "toggle:bold").unwrap();
        assert_eq!(actions[bold + 1], "toggle:italic");
        assert!(actions.contains(&"insert-element:bulletedList".to_string()));
        let ids: std::collections::HashSet<_> = bar.iter().map(|e| &e.id).collect();
        assert_eq!(ids.len(), bar.len(), "ids are unique");
    }

    #[test]
    fn empty_behaviour_has_no_groups() {
        let bar = default_toolbar(&Behaviour::default());
        assert!(bar.iter().all(|e| e.action != Action::Separator));
    }
}
