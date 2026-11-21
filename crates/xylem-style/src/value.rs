//! CSS component values: grouping of tokens plus colour and length parsing.

use crate::token::Token;
use crate::types::Color;

/// Maximum nesting of functions inside a value; deeper input is truncated.
const MAX_NESTING: usize = 16;

/// A component value (whitespace removed, functions grouped).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Cv {
    Ident(String),
    Str(String),
    Number(f32),
    Percentage(f32),
    Dimension(f32, String),
    Hash(String),
    Url(String),
    Function(String, Vec<Cv>),
    Comma,
    Delim(char),
}

impl Cv {
    pub(crate) fn is_ident(&self, keyword: &str) -> bool {
        matches!(self, Cv::Ident(s) if s.eq_ignore_ascii_case(keyword))
    }

    /// The identifier text, lowercased.
    pub(crate) fn keyword(&self) -> Option<String> {
        match self {
            Cv::Ident(s) => Some(s.to_ascii_lowercase()),
            _ => None,
        }
    }
}

/// Groups `tokens` into component values.
pub(crate) fn components(tokens: &[Token]) -> Vec<Cv> {
    let mut pos = 0;
    parse_sequence(tokens, &mut pos, 0)
}

fn parse_sequence(tokens: &[Token], pos: &mut usize, depth: usize) -> Vec<Cv> {
    let mut out = Vec::new();
    while let Some(token) = tokens.get(*pos) {
        *pos += 1;
        let cv = match token {
            Token::Whitespace => continue,
            Token::RParen if depth > 0 => return out,
            Token::Function(name) => {
                let args = if depth < MAX_NESTING {
                    parse_sequence(tokens, pos, depth + 1)
                } else {
                    skip_group(tokens, pos);
                    Vec::new()
                };
                Cv::Function(name.to_ascii_lowercase(), args)
            }
            Token::Ident(s) => Cv::Ident(s.clone()),
            Token::Str(s) => Cv::Str(s.clone()),
            Token::Number(n, _) => Cv::Number(*n),
            Token::Percentage(n) => Cv::Percentage(*n),
            Token::Dimension(n, unit) => Cv::Dimension(*n, unit.clone()),
            Token::Hash(s) => Cv::Hash(s.clone()),
            Token::Url(s) => Cv::Url(s.clone()),
            Token::Comma => Cv::Comma,
            Token::Delim(c) => Cv::Delim(*c),
            Token::Colon => Cv::Delim(':'),
            Token::Semicolon => Cv::Delim(';'),
            Token::MatchOp(c) => Cv::Delim(*c),
            Token::AtKeyword(_)
            | Token::LBrace
            | Token::RBrace
            | Token::LBracket
            | Token::RBracket
            | Token::LParen
            | Token::RParen => Cv::Delim('?'),
        };
        out.push(cv);
    }
    out
}

fn skip_group(tokens: &[Token], pos: &mut usize) {
    let mut depth = 1;
    while let Some(token) = tokens.get(*pos) {
        *pos += 1;
        match token {
            Token::Function(_) | Token::LParen => depth += 1,
            Token::RParen => {
                depth -= 1;
                if depth == 0 {
                    return;
                }
            }
            _ => {}
        }
    }
}

// ----------------------------------------------------------------- lengths

/// An unresolved CSS length.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) enum Length {
    Px(f32),
    Em(f32),
    Percent(f32),
    Auto,
}

impl Length {
    /// Resolves to px given the element's font size and a percentage base.
    pub(crate) fn resolve(self, font_size: f32, percent_base: f32) -> f32 {
        match self {
            Length::Px(v) => v,
            Length::Em(v) => v * font_size,
            Length::Percent(p) => p / 100.0 * percent_base,
            Length::Auto => 0.0,
        }
    }
}

const PX_PER_PT: f32 = 96.0 / 72.0;

/// Parses a single length, percentage or (lenient) bare number.
pub(crate) fn parse_length(cv: &Cv) -> Option<Length> {
    match cv {
        Cv::Number(n) => Some(Length::Px(*n)),
        Cv::Percentage(p) => Some(Length::Percent(*p)),
        Cv::Dimension(v, unit) => {
            let v = *v;
            Some(match unit.as_str() {
                "px" => Length::Px(v),
                "pt" => Length::Px(v * PX_PER_PT),
                "pc" => Length::Px(v * 16.0),
                "in" => Length::Px(v * 96.0),
                "cm" => Length::Px(v * 96.0 / 2.54),
                "mm" => Length::Px(v * 96.0 / 25.4),
                "q" => Length::Px(v * 96.0 / 101.6),
                "em" => Length::Em(v),
                "ex" => Length::Em(v * 0.5),
                "rem" => Length::Px(v * 16.0),
                _ => return None,
            })
        }
        Cv::Ident(s) if s.eq_ignore_ascii_case("auto") => Some(Length::Auto),
        _ => None,
    }
}

// ------------------------------------------------------------------ colours

/// A parsed colour. `CurrentColor` is distinct from every concrete value.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) enum ColorValue {
    CurrentColor,
    Rgba(Color),
}

impl ColorValue {
    /// `None` for `currentColor`.
    pub(crate) fn concrete(self) -> Option<Color> {
        match self {
            ColorValue::CurrentColor => None,
            ColorValue::Rgba(c) => Some(c),
        }
    }
}

pub(crate) fn parse_color(cv: &Cv) -> Option<ColorValue> {
    match cv {
        Cv::Hash(h) => parse_hex(h).map(ColorValue::Rgba),
        Cv::Ident(name) => {
            let name = name.to_ascii_lowercase();
            match name.as_str() {
                "transparent" => Some(ColorValue::Rgba([0, 0, 0, 0])),
                "currentcolor" => Some(ColorValue::CurrentColor),
                _ => named_color(&name).map(ColorValue::Rgba),
            }
        }
        Cv::Function(name, args) if name == "rgb" || name == "rgba" => parse_rgb(args),
        _ => None,
    }
}

fn parse_hex(hex: &str) -> Option<Color> {
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let digit = |i: usize| u8::from_str_radix(&hex[i..=i], 16).ok().map(|d| d * 17);
    let pair = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    match hex.len() {
        3 => Some([digit(0)?, digit(1)?, digit(2)?, 255]),
        4 => Some([digit(0)?, digit(1)?, digit(2)?, digit(3)?]),
        6 => Some([pair(0)?, pair(2)?, pair(4)?, 255]),
        8 => Some([pair(0)?, pair(2)?, pair(4)?, pair(6)?]),
        _ => None,
    }
}

fn parse_rgb(args: &[Cv]) -> Option<ColorValue> {
    let numbers: Vec<&Cv> = args
        .iter()
        .filter(|a| !matches!(a, Cv::Comma | Cv::Delim('/')))
        .collect();
    if numbers.len() != 3 && numbers.len() != 4 {
        return None;
    }
    let channel = |cv: &Cv| match cv {
        Cv::Number(n) => Some(n.round().clamp(0.0, 255.0) as u8),
        Cv::Percentage(p) => Some((p / 100.0 * 255.0).round().clamp(0.0, 255.0) as u8),
        _ => None,
    };
    let alpha = |cv: &Cv| match cv {
        Cv::Number(n) => Some((n.clamp(0.0, 1.0) * 255.0).round() as u8),
        Cv::Percentage(p) => Some((p / 100.0 * 255.0).round().clamp(0.0, 255.0) as u8),
        _ => None,
    };
    let a = match numbers.get(3) {
        Some(cv) => alpha(cv)?,
        None => 255,
    };
    Some(ColorValue::Rgba([
        channel(numbers[0])?,
        channel(numbers[1])?,
        channel(numbers[2])?,
        a,
    ]))
}

/// CSS named colours (lowercase) with their 24-bit RGB values.
const NAMED_COLORS: &[(&str, u32)] = &[
    ("aliceblue", 0xf0f8ff),
    ("antiquewhite", 0xfaebd7),
    ("aqua", 0x00ffff),
    ("aquamarine", 0x7fffd4),
    ("azure", 0xf0ffff),
    ("beige", 0xf5f5dc),
    ("bisque", 0xffe4c4),
    ("black", 0x000000),
    ("blanchedalmond", 0xffebcd),
    ("blue", 0x0000ff),
    ("blueviolet", 0x8a2be2),
    ("brown", 0xa52a2a),
    ("burlywood", 0xdeb887),
    ("cadetblue", 0x5f9ea0),
    ("chartreuse", 0x7fff00),
    ("chocolate", 0xd2691e),
    ("coral", 0xff7f50),
    ("cornflowerblue", 0x6495ed),
    ("cornsilk", 0xfff8dc),
    ("crimson", 0xdc143c),
    ("cyan", 0x00ffff),
    ("darkblue", 0x00008b),
    ("darkcyan", 0x008b8b),
    ("darkgoldenrod", 0xb8860b),
    ("darkgray", 0xa9a9a9),
    ("darkgreen", 0x006400),
    ("darkgrey", 0xa9a9a9),
    ("darkkhaki", 0xbdb76b),
    ("darkmagenta", 0x8b008b),
    ("darkolivegreen", 0x556b2f),
    ("darkorange", 0xff8c00),
    ("darkorchid", 0x9932cc),
    ("darkred", 0x8b0000),
    ("darksalmon", 0xe9967a),
    ("darkseagreen", 0x8fbc8f),
    ("darkslateblue", 0x483d8b),
    ("darkslategray", 0x2f4f4f),
    ("darkslategrey", 0x2f4f4f),
    ("darkturquoise", 0x00ced1),
    ("darkviolet", 0x9400d3),
    ("deeppink", 0xff1493),
    ("deepskyblue", 0x00bfff),
    ("dimgray", 0x696969),
    ("dimgrey", 0x696969),
    ("dodgerblue", 0x1e90ff),
    ("firebrick", 0xb22222),
    ("floralwhite", 0xfffaf0),
    ("forestgreen", 0x228b22),
    ("fuchsia", 0xff00ff),
    ("gainsboro", 0xdcdcdc),
    ("ghostwhite", 0xf8f8ff),
    ("gold", 0xffd700),
    ("goldenrod", 0xdaa520),
    ("gray", 0x808080),
    ("green", 0x008000),
    ("greenyellow", 0xadff2f),
    ("grey", 0x808080),
    ("honeydew", 0xf0fff0),
    ("hotpink", 0xff69b4),
    ("indianred", 0xcd5c5c),
    ("indigo", 0x4b0082),
    ("ivory", 0xfffff0),
    ("khaki", 0xf0e68c),
    ("lavender", 0xe6e6fa),
    ("lavenderblush", 0xfff0f5),
    ("lawngreen", 0x7cfc00),
    ("lemonchiffon", 0xfffacd),
    ("lightblue", 0xadd8e6),
    ("lightcoral", 0xf08080),
    ("lightcyan", 0xe0ffff),
    ("lightgoldenrodyellow", 0xfafad2),
    ("lightgray", 0xd3d3d3),
    ("lightgreen", 0x90ee90),
    ("lightgrey", 0xd3d3d3),
    ("lightpink", 0xffb6c1),
    ("lightsalmon", 0xffa07a),
    ("lightseagreen", 0x20b2aa),
    ("lightskyblue", 0x87cefa),
    ("lightslategray", 0x778899),
    ("lightslategrey", 0x778899),
    ("lightsteelblue", 0xb0c4de),
    ("lightyellow", 0xffffe0),
    ("lime", 0x00ff00),
    ("limegreen", 0x32cd32),
    ("linen", 0xfaf0e6),
    ("magenta", 0xff00ff),
    ("maroon", 0x800000),
    ("mediumaquamarine", 0x66cdaa),
    ("mediumblue", 0x0000cd),
    ("mediumorchid", 0xba55d3),
    ("mediumpurple", 0x9370db),
    ("mediumseagreen", 0x3cb371),
    ("mediumslateblue", 0x7b68ee),
    ("mediumspringgreen", 0x00fa9a),
    ("mediumturquoise", 0x48d1cc),
    ("mediumvioletred", 0xc71585),
    ("midnightblue", 0x191970),
    ("mintcream", 0xf5fffa),
    ("mistyrose", 0xffe4e1),
    ("moccasin", 0xffe4b5),
    ("navajowhite", 0xffdead),
    ("navy", 0x000080),
    ("oldlace", 0xfdf5e6),
    ("olive", 0x808000),
    ("olivedrab", 0x6b8e23),
    ("orange", 0xffa500),
    ("orangered", 0xff4500),
    ("orchid", 0xda70d6),
    ("palegoldenrod", 0xeee8aa),
    ("palegreen", 0x98fb98),
    ("paleturquoise", 0xafeeee),
    ("palevioletred", 0xdb7093),
    ("papayawhip", 0xffefd5),
    ("peachpuff", 0xffdab9),
    ("peru", 0xcd853f),
    ("pink", 0xffc0cb),
    ("plum", 0xdda0dd),
    ("powderblue", 0xb0e0e6),
    ("purple", 0x800080),
    ("rebeccapurple", 0x663399),
    ("red", 0xff0000),
    ("rosybrown", 0xbc8f8f),
    ("royalblue", 0x4169e1),
    ("saddlebrown", 0x8b4513),
    ("salmon", 0xfa8072),
    ("sandybrown", 0xf4a460),
    ("seagreen", 0x2e8b57),
    ("seashell", 0xfff5ee),
    ("sienna", 0xa0522d),
    ("silver", 0xc0c0c0),
    ("skyblue", 0x87ceeb),
    ("slateblue", 0x6a5acd),
    ("slategray", 0x708090),
    ("slategrey", 0x708090),
    ("snow", 0xfffafa),
    ("springgreen", 0x00ff7f),
    ("steelblue", 0x4682b4),
    ("tan", 0xd2b48c),
    ("teal", 0x008080),
    ("thistle", 0xd8bfd8),
    ("tomato", 0xff6347),
    ("turquoise", 0x40e0d0),
    ("violet", 0xee82ee),
    ("wheat", 0xf5deb3),
    ("white", 0xffffff),
    ("whitesmoke", 0xf5f5f5),
    ("yellow", 0xffff00),
    ("yellowgreen", 0x9acd32),
    // System colours commonly seen in XMetaL stylesheets, mapped to the
    // classic Windows palette.
    ("windowtext", 0x000000),
    ("window", 0xffffff),
    ("buttonface", 0xf0f0f0),
    ("graytext", 0x808080),
    ("highlight", 0x3399ff),
    ("highlighttext", 0xffffff),
];

fn named_color(lowercase_name: &str) -> Option<Color> {
    NAMED_COLORS
        .iter()
        .find(|(n, _)| *n == lowercase_name)
        .map(|&(_, rgb)| [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 255])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::tokenize;

    fn cvs(src: &str) -> Vec<Cv> {
        components(&tokenize(src).tokens)
    }

    fn color(src: &str) -> Option<ColorValue> {
        parse_color(&cvs(src)[0])
    }

    #[test]
    fn hex_colours() {
        assert_eq!(
            color("#ebffeb"),
            Some(ColorValue::Rgba([0xeb, 0xff, 0xeb, 255]))
        );
        assert_eq!(color("#0f8"), Some(ColorValue::Rgba([0, 0xff, 0x88, 255])));
        assert_eq!(color("#12345"), None);
    }

    #[test]
    fn named_colours_are_case_insensitive() {
        assert_eq!(color("Red"), Some(ColorValue::Rgba([255, 0, 0, 255])));
        assert_eq!(color("DARKGREEN"), Some(ColorValue::Rgba([0, 100, 0, 255])));
        assert_eq!(color("transparent"), Some(ColorValue::Rgba([0, 0, 0, 0])));
        assert_eq!(color("currentColor"), Some(ColorValue::CurrentColor));
        assert_eq!(color("nonsense"), None);
    }

    #[test]
    fn rgb_functions() {
        assert_eq!(
            color("rgb(10, 20, 30)"),
            Some(ColorValue::Rgba([10, 20, 30, 255]))
        );
        assert_eq!(
            color("rgb(100%, 0%, 50%)"),
            Some(ColorValue::Rgba([255, 0, 128, 255]))
        );
        assert_eq!(
            color("rgba(0,0,0,0.5)"),
            Some(ColorValue::Rgba([0, 0, 0, 128]))
        );
        assert_eq!(color("rgb(1,2)"), None);
    }

    #[test]
    fn lengths() {
        let len = |s: &str| parse_length(&cvs(s)[0]);
        assert_eq!(len("12px"), Some(Length::Px(12.0)));
        assert_eq!(len("12pt"), Some(Length::Px(16.0)));
        assert_eq!(len("1.5em"), Some(Length::Em(1.5)));
        assert_eq!(len("50%"), Some(Length::Percent(50.0)));
        assert_eq!(len("1in"), Some(Length::Px(96.0)));
        assert_eq!(len("3furlongs"), None);
    }

    #[test]
    fn nested_functions() {
        let v = cvs("counter(a, upper-roman) 'x'");
        assert_eq!(
            v,
            vec![
                Cv::Function(
                    "counter".into(),
                    vec![
                        Cv::Ident("a".into()),
                        Cv::Comma,
                        Cv::Ident("upper-roman".into())
                    ]
                ),
                Cv::Str("x".into())
            ]
        );
    }
}
