//! Declarations: parsing property values into typed form, and applying them
//! to a style under construction.

use crate::token::Token;
use crate::types::{
    BorderStyle, Color, ComputedStyle, Display, ListStyle, Side, TextAlign, TextTransform,
    VerticalAlign, WhiteSpace,
};
use crate::value::{ColorValue, Cv, Length, components, parse_color, parse_length};

// ------------------------------------------------------------------- model

/// One item of a `content` value.
#[derive(Clone, Debug, PartialEq)]
pub enum ContentItem {
    Str(String),
    Attr(String),
    Counter {
        name: String,
        style: ListStyle,
    },
    Counters {
        name: String,
        separator: String,
        style: ListStyle,
    },
    OpenQuote,
    CloseQuote,
    NoOpenQuote,
    NoCloseQuote,
    /// An image reference.
    Url(String),
}

/// The `content` property.
#[derive(Clone, Debug, PartialEq)]
pub enum ContentValue {
    /// `none` or `normal`: no generated box.
    None,
    Items(Vec<ContentItem>),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum FontSizeValue {
    Length(Length),
    Larger,
    Smaller,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) enum WeightValue {
    Normal,
    Bold,
    Bolder,
    Lighter,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) enum LineHeightValue {
    Normal,
    Factor(f32),
    Length(Length),
}

/// A longhand property with its parsed value. Shorthands are expanded.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Prop {
    Display(Display),
    Margin(Side, Length),
    Padding(Side, Length),
    BorderWidth(Side, Length),
    BorderStyle(Side, BorderStyle),
    /// `None` is `currentColor`.
    BorderColor(Side, Option<Color>),
    Background(Option<Color>),
    Color(Color),
    FontFamily(String),
    FontSize(FontSizeValue),
    FontWeight(WeightValue),
    FontStyle(bool),
    TextDecoration {
        underline: bool,
        line_through: bool,
    },
    TextAlign(TextAlign),
    TextTransform(TextTransform),
    TextIndent(Length),
    LineHeight(LineHeightValue),
    WhiteSpace(WhiteSpace),
    VerticalAlign(VerticalAlign),
    ListStyleType(ListStyle),
    WrapOption(bool),
    Content(ContentValue),
    CounterReset(Vec<(String, i32)>),
    CounterIncrement(Vec<(String, i32)>),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Declaration {
    pub prop: Prop,
    pub important: bool,
}

// ----------------------------------------------------------------- parsing

/// Parses the contents of a declaration block (without the braces). Problems
/// are reported through `warn(line, message)`; bad declarations are dropped.
pub(crate) fn parse_declarations(
    tokens: &[Token],
    lines: &[u32],
    warn: &mut dyn FnMut(u32, String),
) -> Vec<Declaration> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    for i in 0..=tokens.len() {
        let at_end = i == tokens.len();
        if !at_end {
            match &tokens[i] {
                Token::Function(_) | Token::LParen | Token::LBracket | Token::LBrace => depth += 1,
                Token::RParen | Token::RBracket | Token::RBrace => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        if at_end || (depth == 0 && tokens[i] == Token::Semicolon) {
            let line = lines.get(start).copied().unwrap_or(0);
            parse_one(&tokens[start..i], line, &mut out, warn);
            start = i + 1;
        }
    }
    out
}

fn trim_ws(mut tokens: &[Token]) -> &[Token] {
    while let [Token::Whitespace, rest @ ..] = tokens {
        tokens = rest;
    }
    while let [rest @ .., Token::Whitespace] = tokens {
        tokens = rest;
    }
    tokens
}

fn parse_one(
    tokens: &[Token],
    line: u32,
    out: &mut Vec<Declaration>,
    warn: &mut dyn FnMut(u32, String),
) {
    let tokens = trim_ws(tokens);
    if tokens.is_empty() {
        return;
    }
    let (Some(Token::Ident(name)), rest) = (tokens.first(), &tokens[1.min(tokens.len())..]) else {
        warn(line, "malformed declaration".to_string());
        return;
    };
    let rest = trim_ws(rest);
    let [Token::Colon, value @ ..] = rest else {
        warn(line, format!("declaration `{name}` has no colon"));
        return;
    };
    let mut value = trim_ws(value);
    let mut important = false;
    if let [head @ .., Token::Delim('!'), Token::Ident(kw)]
    | [
        head @ ..,
        Token::Delim('!'),
        Token::Whitespace,
        Token::Ident(kw),
    ] = value
        && kw.eq_ignore_ascii_case("important")
    {
        important = true;
        value = trim_ws(head);
    }
    let values = components(value);
    let mut props = Vec::new();
    match parse_property(&name.to_ascii_lowercase(), &values, &mut props) {
        Ok(()) => out.extend(
            props
                .into_iter()
                .map(|prop| Declaration { prop, important }),
        ),
        Err(message) => warn(line, message),
    }
}

fn keyword(values: &[Cv]) -> Option<String> {
    match values {
        [single] => single.keyword(),
        _ => None,
    }
}

/// Expands a 1-4 value box shorthand to (top, right, bottom, left).
fn expand_box<T: Copy>(values: &[T]) -> Option<[T; 4]> {
    match *values {
        [a] => Some([a, a, a, a]),
        [v, h] => Some([v, h, v, h]),
        [t, h, b] => Some([t, h, b, h]),
        [t, r, b, l] => Some([t, r, b, l]),
        _ => None,
    }
}

fn parse_lengths(values: &[Cv]) -> Result<Vec<Length>, String> {
    values
        .iter()
        .map(|v| parse_length(v).ok_or_else(|| format!("invalid length {v:?}")))
        .collect()
}

fn border_style_from(kw: &str) -> Option<BorderStyle> {
    Some(match kw {
        "none" | "hidden" => BorderStyle::None,
        "solid" => BorderStyle::Solid,
        "dashed" => BorderStyle::Dashed,
        "dotted" => BorderStyle::Dotted,
        "double" => BorderStyle::Double,
        "groove" => BorderStyle::Groove,
        "ridge" => BorderStyle::Ridge,
        "inset" => BorderStyle::Inset,
        "outset" => BorderStyle::Outset,
        _ => return None,
    })
}

fn border_width_from(cv: &Cv) -> Option<Length> {
    match cv.keyword().as_deref() {
        Some("thin") => Some(Length::Px(1.0)),
        Some("medium") => Some(Length::Px(3.0)),
        Some("thick") => Some(Length::Px(5.0)),
        Some("auto") => None,
        _ => parse_length(cv),
    }
}

pub(crate) fn list_style_from(kw: &str) -> Option<ListStyle> {
    Some(match kw {
        "none" => ListStyle::None,
        "disc" => ListStyle::Disc,
        "circle" => ListStyle::Circle,
        "square" => ListStyle::Square,
        "decimal" => ListStyle::Decimal,
        "decimal-leading-zero" => ListStyle::DecimalLeadingZero,
        "lower-alpha" | "lower-latin" => ListStyle::LowerAlpha,
        "upper-alpha" | "upper-latin" => ListStyle::UpperAlpha,
        "lower-roman" => ListStyle::LowerRoman,
        "upper-roman" => ListStyle::UpperRoman,
        "lower-greek" => ListStyle::LowerGreek,
        _ => return None,
    })
}

fn display_from(kw: &str) -> Option<Display> {
    Some(match kw {
        "inline" => Display::Inline,
        "block" => Display::Block,
        "list-item" => Display::ListItem,
        // XMetaL's synonym for `none`.
        "none" | "hidden" => Display::None,
        "table" => Display::Table,
        "table-row" => Display::TableRow,
        "table-cell" => Display::TableCell,
        "table-header-group" => Display::TableHeaderGroup,
        "table-row-group" => Display::TableRowGroup,
        "table-footer-group" => Display::TableFooterGroup,
        "table-caption" => Display::TableCaption,
        "table-column" => Display::TableColumn,
        "table-column-group" => Display::TableColumnGroup,
        "inline-block" => Display::InlineBlock,
        "inline-table" => Display::InlineTable,
        _ => return None,
    })
}

/// Border shorthand components: (width, style, colour).
type BorderParts = (Option<Length>, Option<BorderStyle>, Option<ColorValue>);

fn parse_border_parts(values: &[Cv]) -> Result<BorderParts, String> {
    let (mut width, mut style, mut color) = (None, None, None);
    for cv in values {
        if let Some(s) = cv.keyword().as_deref().and_then(border_style_from) {
            style = Some(s);
        } else if let Some(w) = border_width_from(cv) {
            width = Some(w);
        } else if let Some(c) = parse_color(cv) {
            color = Some(c);
        } else {
            return Err(format!("unsupported border component {cv:?}"));
        }
    }
    Ok((width, style, color))
}

fn push_border_side(out: &mut Vec<Prop>, side: Side, parts: BorderParts) {
    let (width, style, color) = parts;
    out.push(Prop::BorderWidth(side, width.unwrap_or(Length::Px(3.0))));
    out.push(Prop::BorderStyle(side, style.unwrap_or(BorderStyle::None)));
    out.push(Prop::BorderColor(
        side,
        color.and_then(ColorValue::concrete),
    ));
}

fn parse_font_family(values: &[Cv]) -> Result<String, String> {
    let mut families = Vec::new();
    for entry in values.split(|v| *v == Cv::Comma) {
        let name = match entry {
            [Cv::Str(s)] => s.clone(),
            idents if !idents.is_empty() => {
                let words: Option<Vec<&str>> = idents
                    .iter()
                    .map(|v| {
                        if let Cv::Ident(s) = v {
                            Some(s.as_str())
                        } else {
                            None
                        }
                    })
                    .collect();
                words.ok_or("invalid font-family")?.join(" ")
            }
            _ => return Err("empty font-family entry".to_string()),
        };
        families.push(name);
    }
    if families.is_empty() {
        return Err("empty font-family".to_string());
    }
    Ok(families.join(", "))
}

fn parse_font_size(values: &[Cv]) -> Result<FontSizeValue, String> {
    if let Some(kw) = keyword(values) {
        let px = match kw.as_str() {
            "xx-small" => 9.0,
            "x-small" => 10.0,
            "small" => 13.0,
            "medium" => 16.0,
            "large" => 18.0,
            "x-large" => 24.0,
            "xx-large" => 32.0,
            "larger" => return Ok(FontSizeValue::Larger),
            "smaller" => return Ok(FontSizeValue::Smaller),
            other => return Err(format!("unsupported font-size `{other}`")),
        };
        return Ok(FontSizeValue::Length(Length::Px(px)));
    }
    match values {
        [v] => parse_length(v)
            .map(FontSizeValue::Length)
            .ok_or_else(|| format!("invalid font-size {v:?}")),
        _ => Err("invalid font-size".to_string()),
    }
}

fn parse_font_weight(values: &[Cv]) -> Result<WeightValue, String> {
    match values {
        [Cv::Number(n)] => Ok(if *n >= 600.0 {
            WeightValue::Bold
        } else {
            WeightValue::Normal
        }),
        [v] => match v.keyword().as_deref() {
            Some("normal") => Ok(WeightValue::Normal),
            Some("bold") => Ok(WeightValue::Bold),
            Some("bolder") => Ok(WeightValue::Bolder),
            Some("lighter") => Ok(WeightValue::Lighter),
            _ => Err(format!("invalid font-weight {v:?}")),
        },
        _ => Err("invalid font-weight".to_string()),
    }
}

fn parse_text_decoration(values: &[Cv]) -> Result<Prop, String> {
    let (mut underline, mut line_through) = (false, false);
    for v in values {
        match v.keyword().as_deref() {
            Some("none") => {}
            Some("underline") => underline = true,
            Some("line-through") => line_through = true,
            Some("overline" | "blink") => {}
            _ => return Err(format!("invalid text-decoration {v:?}")),
        }
    }
    Ok(Prop::TextDecoration {
        underline,
        line_through,
    })
}

fn parse_line_height(values: &[Cv]) -> Result<LineHeightValue, String> {
    match values {
        [Cv::Number(n)] => Ok(LineHeightValue::Factor(*n)),
        [v] if v.is_ident("normal") => Ok(LineHeightValue::Normal),
        [v] => parse_length(v)
            .map(LineHeightValue::Length)
            .ok_or_else(|| format!("invalid line-height {v:?}")),
        _ => Err("invalid line-height".to_string()),
    }
}

fn parse_counter_ops(values: &[Cv], default: i32) -> Result<Vec<(String, i32)>, String> {
    if keyword(values).as_deref() == Some("none") {
        return Ok(Vec::new());
    }
    let mut ops = Vec::new();
    let mut iter = values.iter().peekable();
    while let Some(cv) = iter.next() {
        let Cv::Ident(name) = cv else {
            return Err(format!("invalid counter name {cv:?}"));
        };
        let value = match iter.peek() {
            Some(Cv::Number(n)) => {
                let n = *n as i32;
                iter.next();
                n
            }
            _ => default,
        };
        ops.push((name.clone(), value));
    }
    Ok(ops)
}

fn counter_style(arg: Option<&[Cv]>) -> Result<ListStyle, String> {
    match arg {
        None => Ok(ListStyle::Decimal),
        Some([v]) => v
            .keyword()
            .as_deref()
            .and_then(list_style_from)
            .ok_or_else(|| format!("unknown counter style {v:?}")),
        Some(_) => Err("invalid counter style".to_string()),
    }
}

fn parse_content(values: &[Cv]) -> Result<ContentValue, String> {
    if let Some(kw) = keyword(values)
        && (kw == "none" || kw == "normal")
    {
        return Ok(ContentValue::None);
    }
    let mut items = Vec::new();
    for cv in values {
        let item = match cv {
            Cv::Str(s) => ContentItem::Str(s.clone()),
            Cv::Url(u) => ContentItem::Url(u.clone()),
            Cv::Ident(kw) => match kw.to_ascii_lowercase().as_str() {
                "open-quote" => ContentItem::OpenQuote,
                "close-quote" => ContentItem::CloseQuote,
                "no-open-quote" => ContentItem::NoOpenQuote,
                "no-close-quote" => ContentItem::NoCloseQuote,
                other => return Err(format!("unsupported content keyword `{other}`")),
            },
            Cv::Function(name, args) => {
                let groups: Vec<&[Cv]> = args.split(|a| *a == Cv::Comma).collect();
                let name_of = |group: &[Cv]| match group {
                    [Cv::Ident(n)] | [Cv::Str(n)] => Some(n.clone()),
                    _ => None,
                };
                match name.as_str() {
                    "url" => match args.as_slice() {
                        [Cv::Str(u)] => ContentItem::Url(u.clone()),
                        _ => return Err("invalid url()".to_string()),
                    },
                    // `attr(ns|name)` keeps only the local part.
                    "attr" => {
                        let local = args.iter().rev().find_map(|a| match a {
                            Cv::Ident(n) => Some(n.clone()),
                            _ => None,
                        });
                        ContentItem::Attr(local.ok_or("invalid attr()")?)
                    }
                    "counter" => ContentItem::Counter {
                        name: groups
                            .first()
                            .and_then(|g| name_of(g))
                            .ok_or("invalid counter()")?,
                        style: counter_style(groups.get(1).copied())?,
                    },
                    "counters" => ContentItem::Counters {
                        name: groups
                            .first()
                            .and_then(|g| name_of(g))
                            .ok_or("invalid counters()")?,
                        separator: match groups.get(1) {
                            Some([Cv::Str(s)]) => s.clone(),
                            _ => return Err("invalid counters() separator".to_string()),
                        },
                        style: counter_style(groups.get(2).copied())?,
                    },
                    other => return Err(format!("unsupported content function `{other}()`")),
                }
            }
            other => return Err(format!("invalid content value {other:?}")),
        };
        items.push(item);
    }
    if items.is_empty() {
        return Err("empty content".to_string());
    }
    Ok(ContentValue::Items(items))
}

fn parse_color_single(values: &[Cv]) -> Result<ColorValue, String> {
    match values {
        [v] => parse_color(v).ok_or_else(|| format!("unsupported colour {v:?}")),
        _ => Err("invalid colour".to_string()),
    }
}

fn sides_of<T: Copy>(values: &[T], what: &str) -> Result<[T; 4], String> {
    expand_box(values).ok_or_else(|| format!("{what} takes 1-4 values"))
}

/// Parses `name: values` into zero or more longhand properties.
pub(crate) fn parse_property(name: &str, values: &[Cv], out: &mut Vec<Prop>) -> Result<(), String> {
    if values.is_empty() {
        return Err(format!("`{name}` has no value"));
    }
    if keyword(values).as_deref() == Some("inherit") {
        return Err(format!("`inherit` on `{name}` is not supported"));
    }
    let side_of = |suffix: &str| match suffix {
        "top" => Some(Side::Top),
        "right" => Some(Side::Right),
        "bottom" => Some(Side::Bottom),
        "left" => Some(Side::Left),
        _ => None,
    };
    match name {
        "display" => {
            let kw = keyword(values).ok_or("invalid display")?;
            out.push(Prop::Display(
                display_from(&kw).ok_or_else(|| format!("unsupported display `{kw}`"))?,
            ));
        }
        "margin" | "padding" => {
            let lengths = sides_of(&parse_lengths(values)?, name)?;
            for (side, len) in Side::ALL.into_iter().zip(lengths) {
                out.push(if name == "margin" {
                    Prop::Margin(side, len)
                } else {
                    Prop::Padding(side, len)
                });
            }
        }
        "border" => {
            let parts = parse_border_parts(values)?;
            for side in Side::ALL {
                push_border_side(out, side, parts);
            }
        }
        "border-width" => {
            let widths: Vec<Length> = values
                .iter()
                .map(|v| border_width_from(v).ok_or("invalid border-width"))
                .collect::<Result<_, _>>()?;
            for (side, w) in Side::ALL.into_iter().zip(sides_of(&widths, name)?) {
                out.push(Prop::BorderWidth(side, w));
            }
        }
        "border-style" => {
            let styles: Vec<BorderStyle> = values
                .iter()
                .map(|v| {
                    v.keyword()
                        .as_deref()
                        .and_then(border_style_from)
                        .ok_or("invalid border-style")
                })
                .collect::<Result<_, _>>()?;
            for (side, s) in Side::ALL.into_iter().zip(sides_of(&styles, name)?) {
                out.push(Prop::BorderStyle(side, s));
            }
        }
        "border-color" => {
            let colors: Vec<Option<Color>> = values
                .iter()
                .map(|v| {
                    parse_color(v)
                        .map(ColorValue::concrete)
                        .ok_or("invalid border-color")
                })
                .collect::<Result<_, _>>()?;
            for (side, c) in Side::ALL.into_iter().zip(sides_of(&colors, name)?) {
                out.push(Prop::BorderColor(side, c));
            }
        }
        "background-color" => {
            let color = parse_color_single(values)?;
            out.push(Prop::Background(color.concrete()));
        }
        "background" => {
            let color = values.iter().find_map(parse_color);
            let ignorable = values.iter().all(|v| {
                matches!(
                    v,
                    Cv::Url(_) | Cv::Ident(_) | Cv::Number(_) | Cv::Dimension(..)
                )
            });
            match color {
                Some(c) => out.push(Prop::Background(c.concrete())),
                None if ignorable => out.push(Prop::Background(None)),
                None => return Err("unsupported background".to_string()),
            }
        }
        "color" => match parse_color_single(values)? {
            ColorValue::Rgba(c) => out.push(Prop::Color(c)),
            ColorValue::CurrentColor => {
                return Err("`currentColor` for `color` is not supported".to_string());
            }
        },
        "font-family" => out.push(Prop::FontFamily(parse_font_family(values)?)),
        "font-size" => out.push(Prop::FontSize(parse_font_size(values)?)),
        "font-weight" => out.push(Prop::FontWeight(parse_font_weight(values)?)),
        "font-style" => {
            let kw = keyword(values).ok_or("invalid font-style")?;
            out.push(Prop::FontStyle(match kw.as_str() {
                "normal" => false,
                "italic" | "oblique" => true,
                _ => return Err(format!("unsupported font-style `{kw}`")),
            }));
        }
        "text-decoration" | "text-decoration-line" => out.push(parse_text_decoration(values)?),
        "text-align" => {
            let kw = keyword(values).ok_or("invalid text-align")?;
            out.push(Prop::TextAlign(match kw.as_str() {
                "left" | "start" => TextAlign::Left,
                "right" | "end" => TextAlign::Right,
                "center" => TextAlign::Center,
                "justify" => TextAlign::Justify,
                _ => return Err(format!("unsupported text-align `{kw}`")),
            }));
        }
        "text-transform" => {
            let kw = keyword(values).ok_or("invalid text-transform")?;
            out.push(Prop::TextTransform(match kw.as_str() {
                "none" => TextTransform::None,
                "uppercase" => TextTransform::Uppercase,
                "lowercase" => TextTransform::Lowercase,
                "capitalize" => TextTransform::Capitalize,
                _ => return Err(format!("unsupported text-transform `{kw}`")),
            }));
        }
        "text-indent" => match values {
            [v] => out.push(Prop::TextIndent(
                parse_length(v).ok_or("invalid text-indent")?,
            )),
            _ => return Err("invalid text-indent".to_string()),
        },
        "line-height" => out.push(Prop::LineHeight(parse_line_height(values)?)),
        "white-space" => {
            let kw = keyword(values).ok_or("invalid white-space")?;
            out.push(Prop::WhiteSpace(match kw.as_str() {
                "normal" => WhiteSpace::Normal,
                "pre" => WhiteSpace::Pre,
                "nowrap" => WhiteSpace::Nowrap,
                "pre-wrap" => WhiteSpace::PreWrap,
                "pre-line" => WhiteSpace::PreLine,
                _ => return Err(format!("unsupported white-space `{kw}`")),
            }));
        }
        "vertical-align" => {
            let kw = keyword(values).ok_or("unsupported vertical-align value")?;
            out.push(Prop::VerticalAlign(match kw.as_str() {
                "baseline" => VerticalAlign::Baseline,
                "super" => VerticalAlign::Super,
                "sub" => VerticalAlign::Sub,
                "top" | "text-top" => VerticalAlign::Top,
                "middle" => VerticalAlign::Middle,
                "bottom" | "text-bottom" => VerticalAlign::Bottom,
                _ => return Err(format!("unsupported vertical-align `{kw}`")),
            }));
        }
        "list-style-type" => {
            let kw = keyword(values).ok_or("invalid list-style-type")?;
            out.push(Prop::ListStyleType(
                list_style_from(&kw)
                    .ok_or_else(|| format!("unsupported list-style-type `{kw}`"))?,
            ));
        }
        "list-style" => {
            let style = values
                .iter()
                .find_map(|v| v.keyword().as_deref().and_then(list_style_from));
            out.push(Prop::ListStyleType(
                style.ok_or("list-style without a type")?,
            ));
        }
        // XMetaL: `wrap-option: wrap | no-wrap | emergency`.
        "wrap-option" => {
            let kw = keyword(values).ok_or("invalid wrap-option")?;
            out.push(Prop::WrapOption(kw == "emergency"));
        }
        "content" => out.push(Prop::Content(parse_content(values)?)),
        "counter-reset" => out.push(Prop::CounterReset(parse_counter_ops(values, 0)?)),
        "counter-increment" => out.push(Prop::CounterIncrement(parse_counter_ops(values, 1)?)),
        _ => {
            if let Some(suffix) = name
                .strip_prefix("margin-")
                .or_else(|| name.strip_prefix("padding-"))
                && let Some(side) = side_of(suffix)
            {
                let len = match values {
                    [v] => parse_length(v).ok_or_else(|| format!("invalid length {v:?}"))?,
                    _ => return Err(format!("`{name}` takes one value")),
                };
                out.push(if name.starts_with("margin") {
                    Prop::Margin(side, len)
                } else {
                    Prop::Padding(side, len)
                });
                return Ok(());
            }
            if let Some(rest) = name.strip_prefix("border-") {
                let (side_name, aspect) = rest.split_once('-').unwrap_or((rest, ""));
                if let Some(side) = side_of(side_name) {
                    match aspect {
                        "" => push_border_side(out, side, parse_border_parts(values)?),
                        "width" => match values {
                            [v] => out.push(Prop::BorderWidth(
                                side,
                                border_width_from(v).ok_or("invalid border width")?,
                            )),
                            _ => return Err(format!("`{name}` takes one value")),
                        },
                        "style" => {
                            let kw = keyword(values)
                                .and_then(|k| border_style_from(&k))
                                .ok_or("invalid border style")?;
                            out.push(Prop::BorderStyle(side, kw));
                        }
                        "color" => out.push(Prop::BorderColor(
                            side,
                            parse_color_single(values)?.concrete(),
                        )),
                        _ => return Err(format!("unsupported property `{name}`")),
                    }
                    return Ok(());
                }
            }
            return Err(format!("unsupported property `{name}`"));
        }
    }
    Ok(())
}

// ------------------------------------------------------------- application

/// A style being cascaded, plus the non-style outputs of the cascade.
pub(crate) struct Working {
    pub style: ComputedStyle,
    pub content: Option<ContentValue>,
    pub counter_reset: Vec<(String, i32)>,
    pub counter_increment: Vec<(String, i32)>,
    /// Border widths before `border-style: none` zeroes them.
    border_width: [f32; 4],
}

const MEDIUM_BORDER: f32 = 3.0;
const BIGGER_SMALLER_RATIO: f32 = 1.2;

impl Working {
    /// Starts from the inherited style of `parent`, with `display` as the
    /// initial display value.
    pub(crate) fn new(parent: &ComputedStyle, display: Display) -> Self {
        let mut style = ComputedStyle::inherit_from(parent);
        style.display = display;
        Self {
            style,
            content: None,
            counter_reset: Vec::new(),
            counter_increment: Vec::new(),
            border_width: [MEDIUM_BORDER; 4],
        }
    }

    /// Resolves values that depend on the final font size or border style.
    pub(crate) fn finish(mut self) -> Self {
        for side in Side::ALL {
            let border = self.style.border.side_mut(side);
            border.width = if border.style == BorderStyle::None {
                0.0
            } else {
                self.border_width[side.index()]
            };
        }
        if let Some(factor) = self.style.line_height_factor {
            self.style.line_height = Some(factor * self.style.font_size);
        }
        self
    }
}

impl Prop {
    /// Applies the font-size property; other properties need the final value.
    pub(crate) fn apply_font_size(&self, w: &mut Working, parent: &ComputedStyle) {
        if let Prop::FontSize(size) = self {
            let parent_px = parent.font_size;
            w.style.font_size = match size {
                FontSizeValue::Length(len) => len.resolve(parent_px, parent_px),
                FontSizeValue::Larger => parent_px * BIGGER_SMALLER_RATIO,
                FontSizeValue::Smaller => parent_px / BIGGER_SMALLER_RATIO,
            };
        }
    }

    /// Applies any property except font-size.
    pub(crate) fn apply(&self, w: &mut Working) {
        let font = w.style.font_size;
        let s = &mut w.style;
        match self {
            Prop::FontSize(_) => {}
            Prop::Display(d) => s.display = *d,
            Prop::Margin(side, len) => s.margin.set(*side, len.resolve(font, 0.0)),
            Prop::Padding(side, len) => s.padding.set(*side, len.resolve(font, 0.0)),
            Prop::BorderWidth(side, len) => w.border_width[side.index()] = len.resolve(font, 0.0),
            Prop::BorderStyle(side, style) => s.border.side_mut(*side).style = *style,
            Prop::BorderColor(side, color) => s.border.side_mut(*side).color = *color,
            Prop::Background(color) => s.background = color.filter(|c| c[3] > 0),
            Prop::Color(c) => s.color = Some(*c),
            Prop::FontFamily(family) => {
                s.monospace = is_monospace(family);
                s.font_family = Some(family.clone());
            }
            Prop::FontWeight(weight) => {
                s.bold = match weight {
                    WeightValue::Normal | WeightValue::Lighter => false,
                    WeightValue::Bold | WeightValue::Bolder => true,
                }
            }
            Prop::FontStyle(italic) => s.italic = *italic,
            Prop::TextDecoration {
                underline,
                line_through,
            } => {
                s.underline = *underline;
                s.line_through = *line_through;
            }
            Prop::TextAlign(v) => s.text_align = *v,
            Prop::TextTransform(v) => s.text_transform = *v,
            Prop::TextIndent(len) => s.text_indent = len.resolve(font, 0.0),
            Prop::LineHeight(spec) => match spec {
                LineHeightValue::Normal => {
                    s.line_height = None;
                    s.line_height_factor = None;
                }
                LineHeightValue::Factor(f) => s.line_height_factor = Some(*f),
                LineHeightValue::Length(len) => {
                    s.line_height = Some(len.resolve(font, font));
                    s.line_height_factor = None;
                }
            },
            Prop::WhiteSpace(v) => s.white_space = *v,
            Prop::VerticalAlign(v) => s.vertical_align = *v,
            Prop::ListStyleType(v) => s.list_style = *v,
            Prop::WrapOption(v) => s.wrap_emergency = *v,
            Prop::Content(v) => w.content = Some(v.clone()),
            Prop::CounterReset(v) => w.counter_reset.clone_from(v),
            Prop::CounterIncrement(v) => w.counter_increment.clone_from(v),
        }
    }
}

/// True if the first family is a well-known monospace face, or the list ends
/// in the generic `monospace` fallback.
pub(crate) fn is_monospace(family_list: &str) -> bool {
    const MONO_HINTS: [&str; 6] = [
        "courier",
        "consolas",
        "monospace",
        "lucida console",
        "monaco",
        "menlo",
    ];
    let lower = family_list.to_ascii_lowercase();
    let mut families = lower.split(',').map(str::trim);
    let first_is_mono = families
        .next()
        .is_some_and(|f| MONO_HINTS.iter().any(|h| f.contains(h)));
    first_is_mono
        || lower
            .rsplit(',')
            .next()
            .is_some_and(|last| last.trim() == "monospace")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::tokenize;

    fn parse(css: &str) -> (Vec<Declaration>, Vec<String>) {
        let t = tokenize(css);
        let mut warnings = Vec::new();
        let decls = parse_declarations(&t.tokens, &t.lines, &mut |line, m| {
            warnings.push(format!("{line}: {m}"))
        });
        (decls, warnings)
    }

    fn props(css: &str) -> Vec<Prop> {
        let (decls, warnings) = parse(css);
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
        decls.into_iter().map(|d| d.prop).collect()
    }

    #[test]
    fn margin_shorthand_expands() {
        let p = props("margin: 0 0 .6em 0");
        assert_eq!(p.len(), 4);
        assert_eq!(p[2], Prop::Margin(Side::Bottom, Length::Em(0.6)));
        assert_eq!(
            props("padding: 1px 2px")[3],
            Prop::Padding(Side::Left, Length::Px(2.0))
        );
    }

    #[test]
    fn important_flag() {
        let (d, _) = parse("color: red ! important; display:block");
        assert!(d[0].important);
        assert!(!d[1].important);
    }

    #[test]
    fn border_shorthand_defaults() {
        let p = props("border-top: 2px solid #f00");
        assert_eq!(p[0], Prop::BorderWidth(Side::Top, Length::Px(2.0)));
        assert_eq!(p[1], Prop::BorderStyle(Side::Top, BorderStyle::Solid));
        assert_eq!(p[2], Prop::BorderColor(Side::Top, Some([255, 0, 0, 255])));
        assert_eq!(
            props("border: none")[1],
            Prop::BorderStyle(Side::Top, BorderStyle::None)
        );
    }

    #[test]
    fn hidden_display_is_none() {
        assert_eq!(props("display: hidden"), vec![Prop::Display(Display::None)]);
    }

    #[test]
    fn font_family_and_wrap_option() {
        assert_eq!(
            props("font-family: \"Arial Unicode MS\", Arial, sans-serif"),
            vec![Prop::FontFamily(
                "Arial Unicode MS, Arial, sans-serif".into()
            )]
        );
        assert_eq!(
            props("wrap-option: emergency"),
            vec![Prop::WrapOption(true)]
        );
        assert_eq!(props("wrap-option: wrap"), vec![Prop::WrapOption(false)]);
    }

    #[test]
    fn content_parsing() {
        let p = props(
            r#"content: "\201C" attr(applicability) counter(a, upper-roman) counters(b, ".") open-quote url(x.gif)"#,
        );
        assert_eq!(
            p,
            vec![Prop::Content(ContentValue::Items(vec![
                ContentItem::Str("\u{201c}".into()),
                ContentItem::Attr("applicability".into()),
                ContentItem::Counter {
                    name: "a".into(),
                    style: ListStyle::UpperRoman
                },
                ContentItem::Counters {
                    name: "b".into(),
                    separator: ".".into(),
                    style: ListStyle::Decimal
                },
                ContentItem::OpenQuote,
                ContentItem::Url("x.gif".into()),
            ]))]
        );
        assert_eq!(
            props("content: none"),
            vec![Prop::Content(ContentValue::None)]
        );
    }

    #[test]
    fn counter_ops_defaults() {
        assert_eq!(
            props("counter-reset: a b 5"),
            vec![Prop::CounterReset(vec![("a".into(), 0), ("b".into(), 5)])]
        );
        assert_eq!(
            props("counter-increment: a"),
            vec![Prop::CounterIncrement(vec![("a".into(), 1)])]
        );
        assert_eq!(
            props("counter-reset: none"),
            vec![Prop::CounterReset(vec![])]
        );
    }

    #[test]
    fn bad_declarations_warn_and_are_skipped() {
        let (d, w) = parse("color: red; frobnicate: 3; display: block; ;");
        assert_eq!(d.len(), 2);
        assert_eq!(w.len(), 1);
        assert!(w[0].contains("frobnicate"));
    }

    #[test]
    fn monospace_detection() {
        assert!(is_monospace("Courier New, monospace"));
        assert!(is_monospace("Foo, monospace"));
        assert!(!is_monospace("Arial, sans-serif"));
        assert!(is_monospace("Consolas"));
    }
}
