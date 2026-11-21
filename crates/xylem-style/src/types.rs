//! Computed-style data types handed to the layout engine.

use crate::counter::format_counter;

/// RGBA colour, one byte per channel.
pub type Color = [u8; 4];

/// CSS `display` (XMetaL's `hidden` maps to [`Display::None`]).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum Display {
    #[default]
    Inline,
    Block,
    ListItem,
    None,
    Table,
    TableRow,
    TableCell,
    TableHeaderGroup,
    TableRowGroup,
    TableFooterGroup,
    TableCaption,
    TableColumn,
    TableColumnGroup,
    InlineBlock,
    InlineTable,
}

/// CSS `text-align`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum TextAlign {
    #[default]
    Left,
    Right,
    Center,
    Justify,
}

/// CSS `text-transform`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum TextTransform {
    #[default]
    None,
    Uppercase,
    Lowercase,
    Capitalize,
}

/// CSS `white-space`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum WhiteSpace {
    #[default]
    Normal,
    Pre,
    Nowrap,
    PreWrap,
    PreLine,
}

/// CSS `vertical-align` (keyword values only).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum VerticalAlign {
    #[default]
    Baseline,
    Super,
    Sub,
    Top,
    Middle,
    Bottom,
}

/// `list-style-type` values, also used to format `counter()` output.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum ListStyle {
    None,
    #[default]
    Disc,
    Circle,
    Square,
    Decimal,
    DecimalLeadingZero,
    LowerAlpha,
    UpperAlpha,
    LowerRoman,
    UpperRoman,
    LowerGreek,
}

/// CSS `border-style`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum BorderStyle {
    #[default]
    None,
    Solid,
    Dashed,
    Dotted,
    Double,
    Groove,
    Ridge,
    Inset,
    Outset,
}

/// One border edge. `width` is already 0 when `style` is [`BorderStyle::None`].
#[derive(Copy, Clone, Debug, PartialEq, Default)]
pub struct Border {
    pub width: f32,
    pub style: BorderStyle,
    /// `None` means the element's text colour (`currentColor`).
    pub color: Option<Color>,
}

/// The four border edges of a box.
#[derive(Copy, Clone, Debug, PartialEq, Default)]
pub struct Borders {
    pub top: Border,
    pub right: Border,
    pub bottom: Border,
    pub left: Border,
}

/// A value per box side, in px.
#[derive(Copy, Clone, Debug, PartialEq, Default)]
pub struct Sides {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl Sides {
    /// Left plus right.
    pub fn horizontal(&self) -> f32 {
        self.left + self.right
    }

    /// Top plus bottom.
    pub fn vertical(&self) -> f32 {
        self.top + self.bottom
    }
}

/// A box side, in CSS shorthand order.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Side {
    Top,
    Right,
    Bottom,
    Left,
}

impl Side {
    pub(crate) const ALL: [Side; 4] = [Side::Top, Side::Right, Side::Bottom, Side::Left];

    pub(crate) fn index(self) -> usize {
        self as usize
    }
}

impl Sides {
    pub(crate) fn set(&mut self, side: Side, value: f32) {
        match side {
            Side::Top => self.top = value,
            Side::Right => self.right = value,
            Side::Bottom => self.bottom = value,
            Side::Left => self.left = value,
        }
    }
}

impl Borders {
    pub(crate) fn side_mut(&mut self, side: Side) -> &mut Border {
        match side {
            Side::Top => &mut self.top,
            Side::Right => &mut self.right,
            Side::Bottom => &mut self.bottom,
            Side::Left => &mut self.left,
        }
    }
}

/// Fully resolved style of one node. Lengths are px at 96 dpi.
#[derive(Clone, Debug, PartialEq)]
pub struct ComputedStyle {
    pub display: Display,
    pub font_size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub line_through: bool,
    /// Derived from `font-family` (Courier, Consolas, `monospace`, ...).
    pub monospace: bool,
    /// The `font-family` list as written, comma separated, quotes removed.
    pub font_family: Option<String>,
    /// `None` means "not specified": the renderer's default text colour.
    pub color: Option<Color>,
    /// `None` means transparent.
    pub background: Option<Color>,
    pub text_align: TextAlign,
    pub text_transform: TextTransform,
    pub white_space: WhiteSpace,
    pub vertical_align: VerticalAlign,
    pub margin: Sides,
    pub padding: Sides,
    pub border: Borders,
    pub list_style: ListStyle,
    /// Absolute line height in px; `None` is `normal`.
    pub line_height: Option<f32>,
    pub text_indent: f32,
    /// XMetaL `wrap-option: emergency`.
    pub wrap_emergency: bool,
    /// Unitless `line-height` factor, inherited as a factor and re-resolved
    /// against each descendant's own font size.
    pub(crate) line_height_factor: Option<f32>,
}

impl ComputedStyle {
    /// The initial style of the root: 16px black text, everything else CSS initial.
    pub fn default_root() -> Self {
        Self {
            display: Display::Inline,
            font_size: 16.0,
            bold: false,
            italic: false,
            underline: false,
            line_through: false,
            monospace: false,
            font_family: None,
            color: None,
            background: None,
            text_align: TextAlign::Left,
            text_transform: TextTransform::None,
            white_space: WhiteSpace::Normal,
            vertical_align: VerticalAlign::Baseline,
            margin: Sides::default(),
            padding: Sides::default(),
            border: Borders::default(),
            list_style: ListStyle::Disc,
            line_height: None,
            text_indent: 0.0,
            wrap_emergency: false,
            line_height_factor: None,
        }
    }

    /// The style of a child that matches no rule: inherited properties are
    /// copied, all others take their initial value.
    pub(crate) fn inherit_from(parent: &Self) -> Self {
        Self {
            font_size: parent.font_size,
            bold: parent.bold,
            italic: parent.italic,
            underline: parent.underline,
            line_through: parent.line_through,
            monospace: parent.monospace,
            font_family: parent.font_family.clone(),
            color: parent.color,
            text_align: parent.text_align,
            text_transform: parent.text_transform,
            white_space: parent.white_space,
            list_style: parent.list_style,
            line_height: parent.line_height,
            text_indent: parent.text_indent,
            wrap_emergency: parent.wrap_emergency,
            line_height_factor: parent.line_height_factor,
            ..Self::default_root()
        }
    }
}

impl Default for ComputedStyle {
    fn default() -> Self {
        Self::default_root()
    }
}

/// A resolved `::before` / `::after` box.
#[derive(Clone, Debug, PartialEq)]
pub struct Generated {
    /// Fully resolved text: counters and `attr()` substituted.
    pub text: String,
    /// Image reference from a `url(...)` content item, if any.
    pub image: Option<String>,
    /// Style of the pseudo-element itself.
    pub style: ComputedStyle,
}

impl ListStyle {
    /// Marker text for list item number `value` (without a trailing period).
    pub fn marker(self, value: i32) -> String {
        format_counter(value, self)
    }
}
