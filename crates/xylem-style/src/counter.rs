//! Counter formatting and CSS 2.1 counter scoping.

use std::collections::HashMap;

use xylem_dom::NodeId;

use crate::types::ListStyle;

/// Formats a counter value in the given list style (as `counter(name, style)`
/// and `display: list-item` markers do). No trailing period is added.
///
/// Alphabetic and roman styles fall back to decimal for values they cannot
/// express (zero, negatives, roman numbers beyond 3999).
pub fn format_counter(value: i32, style: ListStyle) -> String {
    match style {
        ListStyle::None => String::new(),
        ListStyle::Disc => "\u{2022}".to_string(),
        ListStyle::Circle => "\u{25e6}".to_string(),
        ListStyle::Square => "\u{25aa}".to_string(),
        ListStyle::Decimal => value.to_string(),
        ListStyle::DecimalLeadingZero => {
            if value < 0 {
                format!("-{:02}", value.unsigned_abs())
            } else {
                format!("{value:02}")
            }
        }
        ListStyle::LowerAlpha => alphabetic(value, 'a', 26).unwrap_or_else(|| value.to_string()),
        ListStyle::UpperAlpha => alphabetic(value, 'A', 26).unwrap_or_else(|| value.to_string()),
        ListStyle::LowerGreek => {
            alphabetic(value, '\u{3b1}', 24).unwrap_or_else(|| value.to_string())
        }
        ListStyle::LowerRoman => {
            roman(value).map_or_else(|| value.to_string(), |s| s.to_lowercase())
        }
        ListStyle::UpperRoman => roman(value).unwrap_or_else(|| value.to_string()),
    }
}

/// Bijective base-`radix` numbering: 1 = a, 26 = z, 27 = aa. `first` must be
/// followed by `radix - 1` consecutive code points that are valid chars.
fn alphabetic(value: i32, first: char, radix: u32) -> Option<String> {
    if value < 1 {
        return None;
    }
    let mut n = value as u32;
    let mut digits = Vec::new();
    while n > 0 {
        n -= 1;
        digits.push(char::from_u32(first as u32 + n % radix)?);
        n /= radix;
    }
    Some(digits.iter().rev().collect())
}

fn roman(value: i32) -> Option<String> {
    if !(1..=3999).contains(&value) {
        return None;
    }
    const NUMERALS: [(i32, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut n = value;
    let mut out = String::new();
    for (magnitude, numeral) in NUMERALS {
        while n >= magnitude {
            out.push_str(numeral);
            n -= magnitude;
        }
    }
    Some(out)
}

/// One live counter instance.
struct Instance {
    value: i32,
    /// The node whose subtree bounds this instance's scope: the parent of the
    /// element that created it (the instance is visible to that element, its
    /// following siblings, and all their descendants).
    scope_owner: NodeId,
}

/// Counters in scope at the current point of a document-order walk.
///
/// The caller tracks, per open node, the counter names whose scope it owns,
/// and calls [`CounterScopes::close_scope`] when leaving that node.
#[derive(Default)]
pub(crate) struct CounterScopes {
    stacks: HashMap<String, Vec<Instance>>,
}

impl CounterScopes {
    /// `counter-reset`: creates an instance scoped to `owner`'s subtree, or
    /// replaces one an earlier sibling already created. Returns true if a new
    /// instance was pushed (so the owner must pop it when it closes).
    pub(crate) fn reset(&mut self, name: &str, value: i32, owner: NodeId) -> bool {
        let stack = self.stacks.entry(name.to_string()).or_default();
        match stack.last_mut() {
            Some(top) if top.scope_owner == owner => {
                top.value = value;
                false
            }
            _ => {
                stack.push(Instance {
                    value,
                    scope_owner: owner,
                });
                true
            }
        }
    }

    /// `counter-increment`. A counter not in scope is instantiated (at 0) on
    /// `owner` first. Returns true if a new instance was created.
    pub(crate) fn increment(&mut self, name: &str, by: i32, owner: NodeId) -> bool {
        let created = self.ensure(name, owner);
        if let Some(top) = self.stacks.get_mut(name).and_then(|s| s.last_mut()) {
            top.value = top.value.saturating_add(by);
        }
        created
    }

    /// Instantiates `name` at 0 on `owner` if it is not in scope.
    pub(crate) fn ensure(&mut self, name: &str, owner: NodeId) -> bool {
        if self.stacks.get(name).is_some_and(|s| !s.is_empty()) {
            return false;
        }
        self.reset(name, 0, owner)
    }

    /// Innermost value of `name`, if in scope.
    pub(crate) fn value(&self, name: &str) -> Option<i32> {
        self.stacks
            .get(name)
            .and_then(|s| s.last())
            .map(|i| i.value)
    }

    /// Values from the outermost to the innermost instance of `name`.
    pub(crate) fn values(&self, name: &str) -> impl Iterator<Item = i32> + '_ {
        self.stacks.get(name).into_iter().flatten().map(|i| i.value)
    }

    /// Drops the instance of `name` created in the scope that is closing.
    pub(crate) fn close_scope(&mut self, name: &str) {
        if let Some(stack) = self.stacks.get_mut(name) {
            stack.pop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_and_leading_zero() {
        assert_eq!(format_counter(7, ListStyle::Decimal), "7");
        assert_eq!(format_counter(-3, ListStyle::Decimal), "-3");
        assert_eq!(format_counter(7, ListStyle::DecimalLeadingZero), "07");
        assert_eq!(format_counter(12, ListStyle::DecimalLeadingZero), "12");
    }

    #[test]
    fn alphabetic_styles() {
        assert_eq!(format_counter(1, ListStyle::LowerAlpha), "a");
        assert_eq!(format_counter(26, ListStyle::LowerAlpha), "z");
        assert_eq!(format_counter(27, ListStyle::LowerAlpha), "aa");
        assert_eq!(format_counter(52, ListStyle::UpperAlpha), "AZ");
        assert_eq!(format_counter(0, ListStyle::UpperAlpha), "0");
        assert_eq!(format_counter(1, ListStyle::LowerGreek), "\u{3b1}");
    }

    #[test]
    fn roman_styles() {
        assert_eq!(format_counter(4, ListStyle::UpperRoman), "IV");
        assert_eq!(format_counter(1994, ListStyle::UpperRoman), "MCMXCIV");
        assert_eq!(format_counter(9, ListStyle::LowerRoman), "ix");
        assert_eq!(format_counter(4000, ListStyle::UpperRoman), "4000");
    }

    #[test]
    fn bullets_and_none() {
        assert_eq!(format_counter(3, ListStyle::None), "");
        assert_eq!(format_counter(3, ListStyle::Disc), "\u{2022}");
    }
}
