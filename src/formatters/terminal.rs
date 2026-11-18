//! Terminal ANSI color formatters (`Terminal`, `Terminal256`,
//! `TerminalTrueColor`).

use std::fmt;
use std::sync::LazyLock;

use crate::style::{Color, Style, default_style};
use crate::token::TokenType;

use super::{Formatter, resolve_token_style};

pub(crate) const ANSI_RESET: &str = "\x1b[39;49;00m";

/// Background polarity for [`TerminalFormatter`] and
/// [`super::other::IrcFormatter`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TerminalBackground {
    /// Colors tuned for a light terminal background.
    #[default]
    Light,
    /// Colors tuned for a dark terminal background.
    Dark,
}

/// Look up a Pygments console ANSI escape sequence by color/attribute key.
pub(crate) fn console_code(name: &str) -> Option<&'static str> {
    Some(match name {
        "" => "",
        "reset" => ANSI_RESET,
        "bold" | "white" => "\x1b[01m",
        "faint" => "\x1b[02m",
        "standout" => "\x1b[03m",
        "underline" => "\x1b[04m",
        "blink" => "\x1b[05m",
        "overline" => "\x1b[06m",
        "black" => "\x1b[30m",
        "red" => "\x1b[31m",
        "green" => "\x1b[32m",
        "yellow" => "\x1b[33m",
        "blue" => "\x1b[34m",
        "magenta" => "\x1b[35m",
        "cyan" => "\x1b[36m",
        "gray" => "\x1b[37m",
        "brightblack" => "\x1b[90m",
        "brightred" => "\x1b[91m",
        "brightgreen" => "\x1b[92m",
        "brightyellow" => "\x1b[93m",
        "brightblue" => "\x1b[94m",
        "brightmagenta" => "\x1b[95m",
        "brightcyan" => "\x1b[96m",
        _ => return None,
    })
}

fn ansiformat(out: &mut dyn fmt::Write, mut attr: &str, text: &str) -> fmt::Result {
    if attr.starts_with('+') && attr.ends_with('+') && attr.len() >= 2 {
        out.write_str("\x1b[05m")?;
        attr = &attr[1..attr.len() - 1];
    }
    if attr.starts_with('*') && attr.ends_with('*') && attr.len() >= 2 {
        out.write_str("\x1b[01m")?;
        attr = &attr[1..attr.len() - 1];
    }
    if attr.starts_with('_') && attr.ends_with('_') && attr.len() >= 2 {
        out.write_str("\x1b[04m")?;
        attr = &attr[1..attr.len() - 1];
    }
    if let Some(code) = console_code(attr) {
        out.write_str(code)?;
    }
    out.write_str(text)?;
    out.write_str(ANSI_RESET)
}

fn terminal_scheme(ty: TokenType) -> (&'static str, &'static str) {
    let mut cur = Some(ty);
    while let Some(t) = cur {
        let pair = match t {
            TokenType::TOKEN => ("", ""),
            TokenType::TEXT_WHITESPACE => ("gray", "brightblack"),
            TokenType::COMMENT => ("gray", "brightblack"),
            TokenType::COMMENT_PREPROC => ("cyan", "brightcyan"),
            TokenType::KEYWORD => ("blue", "brightblue"),
            TokenType::KEYWORD_TYPE => ("cyan", "brightcyan"),
            TokenType::OPERATOR_WORD => ("magenta", "brightmagenta"),
            TokenType::NAME_BUILTIN => ("cyan", "brightcyan"),
            TokenType::NAME_FUNCTION => ("green", "brightgreen"),
            TokenType::NAME_NAMESPACE => ("_cyan_", "_brightcyan_"),
            TokenType::NAME_CLASS => ("_green_", "_brightgreen_"),
            TokenType::NAME_EXCEPTION => ("cyan", "brightcyan"),
            TokenType::NAME_DECORATOR => ("brightblack", "gray"),
            TokenType::NAME_VARIABLE | TokenType::NAME_CONSTANT => ("red", "brightred"),
            TokenType::NAME_ATTRIBUTE => ("cyan", "brightcyan"),
            TokenType::NAME_TAG => ("brightblue", "brightblue"),
            TokenType::LITERAL_STRING => ("yellow", "yellow"),
            TokenType::LITERAL_NUMBER => ("blue", "brightblue"),
            TokenType::GENERIC_DELETED => ("brightred", "brightred"),
            TokenType::GENERIC_INSERTED => ("green", "brightgreen"),
            TokenType::GENERIC_HEADING => ("**", "**"),
            TokenType::GENERIC_SUBHEADING => ("*magenta*", "*brightmagenta*"),
            TokenType::GENERIC_PROMPT => ("**", "**"),
            TokenType::GENERIC_ERROR => ("brightred", "brightred"),
            TokenType::ERROR => ("_brightred_", "_brightred_"),
            _ => {
                cur = t.parent();
                continue;
            }
        };
        return pair;
    }
    ("", "")
}

/// Format tokens with 16-color ANSI escape sequences.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TerminalFormatter {
    bg: TerminalBackground,
    linenos: bool,
}

impl TerminalFormatter {
    /// Construct a [`TerminalFormatter`] with a light background and no line numbers.
    #[must_use]
    pub const fn new() -> Self {
        Self { bg: TerminalBackground::Light, linenos: false }
    }

    /// Select light or dark terminal palette colors.
    #[must_use]
    pub const fn with_bg(mut self, bg: TerminalBackground) -> Self {
        self.bg = bg;
        self
    }

    /// Print zero-padded 4-digit line numbers before each line.
    #[must_use]
    pub const fn with_linenos(mut self) -> Self {
        self.linenos = true;
        self
    }
}

impl Formatter for TerminalFormatter {
    fn name(&self) -> &'static str {
        "Terminal"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["terminal", "console"]
    }

    fn filenames(&self) -> &'static [&'static str] {
        &[]
    }

    fn format_tokens(
        &self,
        tokens: &mut dyn Iterator<Item = (TokenType, &str)>,
        out: &mut dyn fmt::Write,
    ) -> fmt::Result {
        let mut lineno = 0usize;
        let write_lineno = |out: &mut dyn fmt::Write, lineno: &mut usize| -> fmt::Result {
            *lineno += 1;
            if *lineno != 1 {
                out.write_char('\n')?;
            }
            write!(out, "{:04}: ", *lineno)
        };
        if self.linenos {
            write_lineno(out, &mut lineno)?;
        }
        for (ty, val) in tokens {
            let pair = terminal_scheme(ty);
            let color = match self.bg {
                TerminalBackground::Light => pair.0,
                TerminalBackground::Dark => pair.1,
            };
            for line in val.split_inclusive('\n') {
                let body = line.strip_suffix('\n').unwrap_or(line);
                if !color.is_empty() {
                    ansiformat(out, color, body)?;
                } else {
                    out.write_str(body)?;
                }
                if line.ends_with('\n') {
                    if self.linenos {
                        write_lineno(out, &mut lineno)?;
                    } else {
                        out.write_char('\n')?;
                    }
                }
            }
        }
        if self.linenos {
            out.write_char('\n')?;
        }
        Ok(())
    }
}

// -- 256-color and TrueColor formatters ------------------------------------------

static XTERM_COLORS: LazyLock<[(u8, u8, u8); 254]> = LazyLock::new(|| {
    let mut table = [(0u8, 0u8, 0u8); 254];
    table[..16].copy_from_slice(&[
        (0x00, 0x00, 0x00),
        (0xcd, 0x00, 0x00),
        (0x00, 0xcd, 0x00),
        (0xcd, 0xcd, 0x00),
        (0x00, 0x00, 0xee),
        (0xcd, 0x00, 0xcd),
        (0x00, 0xcd, 0xcd),
        (0xe5, 0xe5, 0xe5),
        (0x7f, 0x7f, 0x7f),
        (0xff, 0x00, 0x00),
        (0x00, 0xff, 0x00),
        (0xff, 0xff, 0x00),
        (0x5c, 0x5c, 0xff),
        (0xff, 0x00, 0xff),
        (0x00, 0xff, 0xff),
        (0xff, 0xff, 0xff),
    ]);
    let vr = [0x00u8, 0x5f, 0x87, 0xaf, 0xd7, 0xff];
    for (idx, slot) in table[16..233].iter_mut().enumerate() {
        *slot = (vr[(idx / 36) % 6], vr[(idx / 6) % 6], vr[idx % 6]);
    }
    for (idx, slot) in table[233..254].iter_mut().enumerate() {
        let v = 8 + (idx as u8 + 1) * 10;
        *slot = (v, v, v);
    }
    table
});

fn closest_xterm(c: Color) -> u8 {
    let mut best_dist = i32::MAX;
    let mut best_idx = 0u8;
    for (i, &(rx, gx, bx)) in XTERM_COLORS.iter().enumerate() {
        let dr = i32::from(c.r) - i32::from(rx);
        let dg = i32::from(c.g) - i32::from(gx);
        let db = i32::from(c.b) - i32::from(bx);
        let d = dr * dr + dg * dg + db * db;
        if d < best_dist {
            best_dist = d;
            best_idx = i as u8;
        }
    }
    best_idx
}

fn sgr(attrs: &[String]) -> String {
    if attrs.is_empty() { String::new() } else { format!("\x1b[{}m", attrs.join(";")) }
}

fn format_styled_ansi(
    style: &'static Style,
    tokens: &mut dyn Iterator<Item = (TokenType, &str)>,
    out: &mut dyn fmt::Write,
    linenos: bool,
    escape_for: impl Fn(TokenType) -> (String, String),
) -> fmt::Result {
    let mut lineno = 0usize;
    let write_lineno = |out: &mut dyn fmt::Write, lineno: &mut usize| -> fmt::Result {
        *lineno += 1;
        if *lineno != 1 {
            out.write_char('\n')?;
        }
        write!(out, "{:04}: ", *lineno)
    };
    if linenos {
        write_lineno(out, &mut lineno)?;
    }
    for (ty, val) in tokens {
        let resolved = resolve_token_style(style, ty);
        let (on, off) = escape_for(resolved);
        let mut parts = val.split('\n').peekable();
        while let Some(line) = parts.next() {
            if parts.peek().is_some() {
                if !line.is_empty() {
                    write!(out, "{on}{line}{off}")?;
                }
                if linenos {
                    write_lineno(out, &mut lineno)?;
                } else {
                    out.write_char('\n')?;
                }
            } else if !line.is_empty() {
                write!(out, "{on}{line}{off}")?;
            }
        }
    }
    if linenos {
        out.write_char('\n')?;
    }
    Ok(())
}

/// Format tokens using 256-color ANSI SGR escape sequences (`38;5;N`).
#[derive(Clone, Debug)]
pub struct Terminal256Formatter {
    style: &'static Style,
    use_bold: bool,
    use_underline: bool,
    use_italic: bool,
    linenos: bool,
}

impl Default for Terminal256Formatter {
    fn default() -> Self {
        Self::new(default_style())
    }
}

impl Terminal256Formatter {
    /// Construct a [`Terminal256Formatter`] using `style`.
    #[must_use]
    pub const fn new(style: &'static Style) -> Self {
        Self { style, use_bold: true, use_underline: true, use_italic: true, linenos: false }
    }

    /// Disable bold ANSI attributes (`nobold`).
    #[must_use]
    pub const fn without_bold(mut self) -> Self {
        self.use_bold = false;
        self
    }

    /// Disable underline ANSI attributes (`nounderline`).
    #[must_use]
    pub const fn without_underline(mut self) -> Self {
        self.use_underline = false;
        self
    }

    /// Disable italic ANSI attributes (`noitalic`).
    #[must_use]
    pub const fn without_italic(mut self) -> Self {
        self.use_italic = false;
        self
    }

    /// Prefix every line with a 4-digit line number.
    #[must_use]
    pub const fn with_linenos(mut self) -> Self {
        self.linenos = true;
        self
    }

    fn escape_pair(&self, ty: TokenType) -> (String, String) {
        let st = self.style.style_for(ty);
        let mut on = Vec::new();
        let mut off = Vec::new();
        if let Some(c) = st.color {
            on.push("38".into());
            on.push("5".into());
            on.push(closest_xterm(c).to_string());
            off.push("39".into());
        }
        if let Some(bg) = st.background {
            on.push("48".into());
            on.push("5".into());
            on.push(closest_xterm(bg).to_string());
            off.push("49".into());
        }
        let bold = self.use_bold && st.bold;
        let underline = self.use_underline && st.underline;
        let italic = self.use_italic && st.italic;
        if bold {
            on.push("01".into());
        }
        if underline {
            on.push("04".into());
        }
        if italic {
            on.push("03".into());
        }
        if bold || underline || italic {
            off.push("00".into());
        }
        (sgr(&on), sgr(&off))
    }
}

impl Formatter for Terminal256Formatter {
    fn name(&self) -> &'static str {
        "Terminal256"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["terminal256", "console256", "256"]
    }

    fn filenames(&self) -> &'static [&'static str] {
        &[]
    }

    fn format_tokens(
        &self,
        tokens: &mut dyn Iterator<Item = (TokenType, &str)>,
        out: &mut dyn fmt::Write,
    ) -> fmt::Result {
        format_styled_ansi(self.style, tokens, out, self.linenos, |ty| self.escape_pair(ty))
    }
}

/// Format tokens using 24-bit true-color ANSI SGR escape sequences (`38;2;R;G;B`).
#[derive(Clone, Debug)]
pub struct TerminalTrueColorFormatter {
    style: &'static Style,
    use_bold: bool,
    use_underline: bool,
    use_italic: bool,
    linenos: bool,
}

impl Default for TerminalTrueColorFormatter {
    fn default() -> Self {
        Self::new(default_style())
    }
}

impl TerminalTrueColorFormatter {
    /// Construct a [`TerminalTrueColorFormatter`] using `style`.
    #[must_use]
    pub const fn new(style: &'static Style) -> Self {
        Self { style, use_bold: true, use_underline: true, use_italic: true, linenos: false }
    }

    /// Disable bold ANSI attributes (`nobold`).
    #[must_use]
    pub const fn without_bold(mut self) -> Self {
        self.use_bold = false;
        self
    }

    /// Disable underline ANSI attributes (`nounderline`).
    #[must_use]
    pub const fn without_underline(mut self) -> Self {
        self.use_underline = false;
        self
    }

    /// Disable italic ANSI attributes (`noitalic`).
    #[must_use]
    pub const fn without_italic(mut self) -> Self {
        self.use_italic = false;
        self
    }

    /// Prefix every line with a 4-digit line number.
    #[must_use]
    pub const fn with_linenos(mut self) -> Self {
        self.linenos = true;
        self
    }

    fn escape_pair(&self, ty: TokenType) -> (String, String) {
        let st = self.style.style_for(ty);
        let mut on = Vec::new();
        let mut off = Vec::new();
        if let Some(c) = st.color {
            on.push("38".into());
            on.push("2".into());
            on.push(c.r.to_string());
            on.push(c.g.to_string());
            on.push(c.b.to_string());
            off.push("39".into());
        }
        if let Some(bg) = st.background {
            on.push("48".into());
            on.push("2".into());
            on.push(bg.r.to_string());
            on.push(bg.g.to_string());
            on.push(bg.b.to_string());
            off.push("49".into());
        }
        let bold = self.use_bold && st.bold;
        let underline = self.use_underline && st.underline;
        let italic = self.use_italic && st.italic;
        if bold {
            on.push("01".into());
        }
        if underline {
            on.push("04".into());
        }
        if italic {
            on.push("03".into());
        }
        if bold || underline || italic {
            off.push("00".into());
        }
        (sgr(&on), sgr(&off))
    }
}

impl Formatter for TerminalTrueColorFormatter {
    fn name(&self) -> &'static str {
        "TerminalTrueColor"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["terminal16m", "console16m", "16m"]
    }

    fn filenames(&self) -> &'static [&'static str] {
        &[]
    }

    fn format_tokens(
        &self,
        tokens: &mut dyn Iterator<Item = (TokenType, &str)>,
        out: &mut dyn fmt::Write,
    ) -> fmt::Result {
        format_styled_ansi(self.style, tokens, out, self.linenos, |ty| self.escape_pair(ty))
    }
}
