//! Color schemes, converted from every `pygments.styles` style.

use std::collections::HashMap;
use std::fmt;
use std::sync::LazyLock;

use serde::Deserialize;

use crate::token::TokenType;

/// An RGB color.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Color {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
}

impl Color {
    /// Parse `"#rrggbb"`, `"rrggbb"`, `"#rgb"` or `"rgb"`; `None` for
    /// anything else (`"transparent"`, `"inherit"`, ANSI names).
    #[must_use]
    pub fn parse(s: &str) -> Option<Color> {
        let h = s.strip_prefix('#').unwrap_or(s);
        if !h.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let v = u32::from_str_radix(h, 16).ok()?;
        match h.len() {
            6 => Some(Color { r: (v >> 16) as u8, g: (v >> 8) as u8, b: v as u8 }),
            3 => {
                let x = |n: u32| ((n & 0xf) * 0x11) as u8;
                Some(Color { r: x(v >> 8), g: x(v >> 4), b: x(v) })
            }
            _ => None,
        }
    }

    /// Format as six lowercase hex digits without `#` (e.g. `"272822"`).
    #[must_use]
    pub fn to_hex(self) -> String {
        format!("{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

/// How one token type is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TokenStyle {
    /// Foreground color.
    pub color: Option<Color>,
    /// Background color.
    pub background: Option<Color>,
    /// Border color.
    pub border: Option<Color>,
    /// Bold text.
    pub bold: bool,
    /// Italic text.
    pub italic: bool,
    /// Underlined text.
    pub underline: bool,
    /// Roman (serif) font.
    pub roman: bool,
    /// Sans-serif font.
    pub sans: bool,
    /// Monospace font.
    pub mono: bool,
}

/// A color scheme such as `monokai` or `solarized-dark`.
#[derive(Debug)]
pub struct Style {
    /// The style's name.
    pub name: String,
    /// Default background color.
    pub background: Option<Color>,
    /// Background of highlighted lines.
    pub highlight: Option<Color>,
    /// Line number color (`None` means inherit).
    pub line_number_color: Option<Color>,
    /// Line number background (`None` means transparent).
    pub line_number_background: Option<Color>,
    /// Special line number color (`None` means default).
    pub line_number_special_color: Option<Color>,
    /// Special line number background (`None` means transparent).
    pub line_number_special_background: Option<Color>,
    tokens: HashMap<TokenType, TokenStyle>,
    order: Vec<TokenType>,
}

impl Style {
    /// The style for `ty`, inherited from the nearest ancestor that the scheme
    /// styles (Pygments resolves `noinherit` and the `Token` defaults already).
    #[must_use]
    pub fn style_for(&self, ty: TokenType) -> TokenStyle {
        ty.ancestors().find_map(|t| self.tokens.get(&t).copied()).unwrap_or_default()
    }

    /// Whether this scheme defines an entry for `ty`.
    #[must_use]
    pub fn styles_token(&self, ty: TokenType) -> bool {
        self.tokens.contains_key(&ty)
    }

    /// Iterate over `(token_type, style)` pairs in Pygments' `Style._styles` definition order.
    pub fn iter(&self) -> impl Iterator<Item = (TokenType, TokenStyle)> + '_ {
        self.order.iter().filter_map(|&ty| self.tokens.get(&ty).map(|&st| (ty, st)))
    }
}

#[derive(Deserialize)]
struct StyleData {
    name: String,
    background: Option<String>,
    highlight: Option<String>,
    line_number_color: Option<String>,
    line_number_background: Option<String>,
    line_number_special_color: Option<String>,
    line_number_special_background: Option<String>,
    #[serde(default)]
    order: Vec<String>,
    tokens: HashMap<String, EntryData>,
}

#[derive(Deserialize)]
struct EntryData {
    color: Option<String>,
    bg: Option<String>,
    border: Option<String>,
    #[serde(default)]
    bold: bool,
    #[serde(default)]
    italic: bool,
    #[serde(default)]
    underline: bool,
    #[serde(default)]
    roman: bool,
    #[serde(default)]
    sans: bool,
    #[serde(default)]
    mono: bool,
}

static STYLES: LazyLock<Vec<(String, Style)>> = LazyLock::new(|| {
    let raw: std::collections::BTreeMap<String, StyleData> =
        serde_json::from_str(include_str!("../data/styles.json")).expect("bundled styles.json is valid");
    let color = |s: &Option<String>| s.as_deref().and_then(Color::parse);
    raw.into_iter()
        .map(|(key, d)| {
            let tokens: HashMap<TokenType, TokenStyle> = d
                .tokens
                .iter()
                .filter_map(|(name, e)| {
                    let ty = TokenType::from_name(name)?;
                    Some((
                        ty,
                        TokenStyle {
                            color: color(&e.color),
                            background: color(&e.bg),
                            border: color(&e.border),
                            bold: e.bold,
                            italic: e.italic,
                            underline: e.underline,
                            roman: e.roman,
                            sans: e.sans,
                            mono: e.mono,
                        },
                    ))
                })
                .collect();
            let order = if d.order.is_empty() {
                tokens.keys().copied().collect()
            } else {
                d.order.iter().filter_map(|name| TokenType::from_name(name)).collect()
            };
            let style = Style {
                name: d.name.clone(),
                background: color(&d.background),
                highlight: color(&d.highlight),
                line_number_color: color(&d.line_number_color),
                line_number_background: color(&d.line_number_background),
                line_number_special_color: color(&d.line_number_special_color),
                line_number_special_background: color(&d.line_number_special_background),
                tokens,
                order,
            };
            (key, style)
        })
        .collect()
});

/// A bundled style by its Pygments name (`"monokai"`, `"github-dark"`, ...).
#[must_use]
pub fn style_by_name(name: &str) -> Option<&'static Style> {
    STYLES.iter().find(|(k, _)| k == name).map(|(_, s)| s)
}

pub(crate) fn default_style() -> &'static Style {
    style_by_name("default").expect("bundled default style")
}

/// Names of all bundled styles, sorted.
pub fn style_names() -> impl Iterator<Item = &'static str> {
    STYLES.iter().map(|(k, _)| k.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requested_styles_exist() {
        for name in [
            "monokai",
            "dracula",
            "github-dark",
            "solarized-dark",
            "solarized-light",
            "nord",
            "gruvbox-dark",
            "gruvbox-light",
            "one-dark",
            "material",
            "vs",
            "default",
        ] {
            assert!(style_by_name(name).is_some(), "{name}");
        }
    }

    #[test]
    fn monokai_keyword_and_inheritance() {
        let s = style_by_name("monokai").unwrap();
        assert_eq!(s.background, Color::parse("#272822"));
        let kw = s.style_for(TokenType::KEYWORD);
        assert_eq!(kw.color, Color::parse("66d9ef"));
        // A subtype without its own entry inherits.
        assert_eq!(s.style_for(TokenType::KEYWORD_CONSTANT).color, kw.color);
    }

    #[test]
    fn parse_colors() {
        assert_eq!(Color::parse("#fff"), Some(Color { r: 255, g: 255, b: 255 }));
        assert_eq!(Color::parse("transparent"), None);
    }
}
