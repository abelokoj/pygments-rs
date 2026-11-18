//! Smaller markup and plain-text formatters (`Null`, `RawToken`, `Testcase`,
//! `BBCode`, `PangoMarkup`, `IRC`, `Groff`).

use std::collections::BTreeSet;
use std::fmt;

use crate::style::{Style, default_style};
use crate::token::TokenType;

use super::{Formatter, py_repr, resolve_token_style};

// -- NullFormatter ---------------------------------------------------------------

/// Output the token stream text unchanged without any formatting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NullFormatter;

impl NullFormatter {
    /// Construct a [`NullFormatter`].
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Formatter for NullFormatter {
    fn name(&self) -> &'static str {
        "Text only"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["text", "null"]
    }

    fn filenames(&self) -> &'static [&'static str] {
        &["*.txt"]
    }

    fn format_tokens(
        &self,
        tokens: &mut dyn Iterator<Item = (TokenType, &str)>,
        out: &mut dyn fmt::Write,
    ) -> fmt::Result {
        for (_, val) in tokens {
            out.write_str(val)?;
        }
        Ok(())
    }
}

// -- RawTokenFormatter -----------------------------------------------------------

/// Format tokens as `Token.Type\t'repr'\n` lines, round-tripping cleanly with
/// Pygments' `RawTokenLexer`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RawTokenFormatter {
    error_color: Option<String>,
}

impl RawTokenFormatter {
    /// Construct a [`RawTokenFormatter`] with default options.
    #[must_use]
    pub const fn new() -> Self {
        Self { error_color: None }
    }

    /// Highlight [`TokenType::ERROR`] lines with the given console ANSI color name
    /// (for example `"red"` or `"brightred"`).
    ///
    /// Returns `None` if `color` is not a valid Pygments console color name.
    #[must_use]
    pub fn with_error_color(mut self, color: &str) -> Option<Self> {
        super::terminal::console_code(color)?;
        self.error_color = Some(color.to_owned());
        Some(self)
    }
}

impl Formatter for RawTokenFormatter {
    fn name(&self) -> &'static str {
        "Raw tokens"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["raw", "tokens"]
    }

    fn filenames(&self) -> &'static [&'static str] {
        &["*.raw"]
    }

    fn format_tokens(
        &self,
        tokens: &mut dyn Iterator<Item = (TokenType, &str)>,
        out: &mut dyn fmt::Write,
    ) -> fmt::Result {
        for (ty, val) in tokens {
            let repr = py_repr(val, true);
            if ty == TokenType::ERROR
                && let Some(code) = self.error_color.as_deref().and_then(super::terminal::console_code)
            {
                write!(out, "{code}{}\t{repr}\n{}", ty.name(), super::terminal::ANSI_RESET)?;
                continue;
            }
            writeln!(out, "{}\t{repr}", ty.name())?;
        }
        Ok(())
    }
}

// -- TestcaseFormatter -----------------------------------------------------------

/// Format tokens as a Python Pygments unit test (`testNeedsName`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TestcaseFormatter;

impl TestcaseFormatter {
    /// Construct a [`TestcaseFormatter`].
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Formatter for TestcaseFormatter {
    fn name(&self) -> &'static str {
        "Testcase"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["testcase"]
    }

    fn filenames(&self) -> &'static [&'static str] {
        &[]
    }

    fn format_tokens(
        &self,
        tokens: &mut dyn Iterator<Item = (TokenType, &str)>,
        out: &mut dyn fmt::Write,
    ) -> fmt::Result {
        let mut raw = String::new();
        let mut body = String::new();
        for (ty, val) in tokens {
            raw.push_str(val);
            let repr = py_repr(val, false);
            use fmt::Write as _;
            let _ = writeln!(body, "            ({}, {repr}),", ty.name());
        }
        writeln!(out, "    def testNeedsName(lexer):")?;
        writeln!(out, "        fragment = {}", py_repr(&raw, false))?;
        writeln!(out, "        tokens = [")?;
        out.write_str(&body)?;
        writeln!(out, "        ]")?;
        writeln!(out, "        assert list(lexer.get_tokens(fragment)) == tokens")
    }
}

// -- BBCodeFormatter -------------------------------------------------------------

/// Format tokens with BBCode tags (`[color=...]`, `[b]`, `[i]`, `[u]`).
#[derive(Clone, Debug)]
pub struct BBCodeFormatter {
    style: &'static Style,
    codetag: bool,
    monofont: bool,
}

impl Default for BBCodeFormatter {
    fn default() -> Self {
        Self::new(default_style())
    }
}

impl BBCodeFormatter {
    /// Construct a [`BBCodeFormatter`] using `style`.
    #[must_use]
    pub const fn new(style: &'static Style) -> Self {
        Self { style, codetag: false, monofont: false }
    }

    /// Wrap output in `[code]...[/code]`.
    #[must_use]
    pub const fn with_codetag(mut self) -> Self {
        self.codetag = true;
        self
    }

    /// Wrap output in `[font=monospace]...[/font]`.
    #[must_use]
    pub const fn with_monofont(mut self) -> Self {
        self.monofont = true;
        self
    }

    fn tags_for(&self, ty: TokenType) -> (String, String) {
        let st = self.style.style_for(ty);
        let mut start = String::new();
        let mut end = String::new();
        if let Some(c) = st.color {
            start.push_str("[color=#");
            start.push_str(&c.to_hex());
            start.push(']');
            end.insert_str(0, "[/color]");
        }
        if st.bold {
            start.push_str("[b]");
            end.insert_str(0, "[/b]");
        }
        if st.italic {
            start.push_str("[i]");
            end.insert_str(0, "[/i]");
        }
        if st.underline {
            start.push_str("[u]");
            end.insert_str(0, "[/u]");
        }
        (start, end)
    }
}

impl Formatter for BBCodeFormatter {
    fn name(&self) -> &'static str {
        "BBCode"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["bbcode", "bb"]
    }

    fn filenames(&self) -> &'static [&'static str] {
        &[]
    }

    fn format_tokens(
        &self,
        tokens: &mut dyn Iterator<Item = (TokenType, &str)>,
        out: &mut dyn fmt::Write,
    ) -> fmt::Result {
        if self.codetag {
            out.write_str("[code]")?;
        }
        if self.monofont {
            out.write_str("[font=monospace]")?;
        }
        let mut last_val = String::new();
        let mut last_ty: Option<TokenType> = None;
        for (ty, val) in tokens {
            let resolved = resolve_token_style(self.style, ty);
            if Some(resolved) == last_ty {
                last_val.push_str(val);
            } else {
                if !last_val.is_empty()
                    && let Some(lt) = last_ty
                {
                    let (s, e) = self.tags_for(lt);
                    write!(out, "{s}{last_val}{e}")?;
                }
                last_val.clear();
                last_val.push_str(val);
                last_ty = Some(resolved);
            }
        }
        if !last_val.is_empty()
            && let Some(lt) = last_ty
        {
            let (s, e) = self.tags_for(lt);
            write!(out, "{s}{last_val}{e}")?;
        }
        if self.monofont {
            out.write_str("[/font]")?;
        }
        if self.codetag {
            out.write_str("[/code]")?;
        }
        if self.codetag || self.monofont {
            out.write_char('\n')?;
        }
        Ok(())
    }
}

// -- PangoMarkupFormatter --------------------------------------------------------

fn escape_pango(s: &str, out: &mut String) {
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            _ => out.push(ch),
        }
    }
}

/// Format tokens as Pango Markup (`<tt>...</tt>`).
#[derive(Clone, Debug)]
pub struct PangoMarkupFormatter {
    style: &'static Style,
}

impl Default for PangoMarkupFormatter {
    fn default() -> Self {
        Self::new(default_style())
    }
}

impl PangoMarkupFormatter {
    /// Construct a [`PangoMarkupFormatter`] using `style`.
    #[must_use]
    pub const fn new(style: &'static Style) -> Self {
        Self { style }
    }

    fn tags_for(&self, ty: TokenType) -> (String, String) {
        let st = self.style.style_for(ty);
        let mut start = String::new();
        let mut end = String::new();
        if let Some(c) = st.color {
            start.push_str("<span fgcolor=\"");
            start.push_str(&c.to_string());
            start.push_str("\">");
            end.insert_str(0, "</span>");
        }
        if st.bold {
            start.push_str("<b>");
            end.insert_str(0, "</b>");
        }
        if st.italic {
            start.push_str("<i>");
            end.insert_str(0, "</i>");
        }
        if st.underline {
            start.push_str("<u>");
            end.insert_str(0, "</u>");
        }
        (start, end)
    }
}

impl Formatter for PangoMarkupFormatter {
    fn name(&self) -> &'static str {
        "Pango Markup"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["pango", "pangomarkup"]
    }

    fn filenames(&self) -> &'static [&'static str] {
        &[]
    }

    fn format_tokens(
        &self,
        tokens: &mut dyn Iterator<Item = (TokenType, &str)>,
        out: &mut dyn fmt::Write,
    ) -> fmt::Result {
        out.write_str("<tt>")?;
        let mut last_val = String::new();
        let mut last_ty: Option<TokenType> = None;
        for (ty, val) in tokens {
            let resolved = resolve_token_style(self.style, ty);
            if Some(resolved) == last_ty {
                escape_pango(val, &mut last_val);
            } else {
                if !last_val.is_empty()
                    && let Some(lt) = last_ty
                {
                    let (s, e) = self.tags_for(lt);
                    write!(out, "{s}{last_val}{e}")?;
                }
                last_val.clear();
                escape_pango(val, &mut last_val);
                last_ty = Some(resolved);
            }
        }
        if !last_val.is_empty()
            && let Some(lt) = last_ty
        {
            let (s, e) = self.tags_for(lt);
            write!(out, "{s}{last_val}{e}")?;
        }
        out.write_str("</tt>")
    }
}

// -- IrcFormatter ----------------------------------------------------------------

/// Background polarity for terminal and IRC color schemes.
pub use super::terminal::TerminalBackground;

fn irc_color_code(name: &str) -> Option<u8> {
    Some(match name {
        "white" => 0,
        "black" => 1,
        "blue" => 2,
        "brightgreen" => 3,
        "brightred" => 4,
        "yellow" => 5,
        "magenta" => 6,
        "orange" | "green" => 7,
        "brightyellow" => 8,
        "lightgreen" | "brightcyan" => 9,
        "cyan" => 10,
        "lightblue" | "red" => 11,
        "brightblue" => 12,
        "brightmagenta" => 13,
        "brightblack" => 14,
        "gray" => 15,
        _ => return None,
    })
}

fn irc_scheme(ty: TokenType) -> (&'static str, &'static str) {
    let mut cur = Some(ty);
    while let Some(t) = cur {
        let pair = match t {
            TokenType::TOKEN => ("", ""),
            TokenType::TEXT_WHITESPACE => ("gray", "brightblack"),
            TokenType::COMMENT => ("gray", "brightblack"),
            TokenType::COMMENT_PREPROC => ("cyan", "brightcyan"),
            TokenType::KEYWORD => ("blue", "brightblue"),
            TokenType::KEYWORD_TYPE => ("cyan", "brightcyan"),
            TokenType::OPERATOR_WORD => ("magenta", "brightcyan"),
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

fn write_irc_formatted(out: &mut dyn fmt::Write, mut color: &str, text: &str) -> fmt::Result {
    if color.is_empty() {
        return out.write_str(text);
    }
    let mut prefix = String::new();
    let mut suffix = String::new();
    if color.contains('_') {
        prefix.push('\x1d');
        suffix.insert(0, '\x1d');
        color = color.trim_matches('_');
    }
    if color.contains('*') {
        prefix.push('\x02');
        suffix.insert(0, '\x02');
        color = color.trim_matches('*');
    }
    if !color.is_empty()
        && let Some(code) = irc_color_code(color)
    {
        use fmt::Write as _;
        let _ = write!(prefix, "\x03{code:02}");
        suffix.insert(0, '\x03');
    }
    write!(out, "{prefix}{text}{suffix}")
}

/// Format tokens with IRC color control codes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IrcFormatter {
    bg: TerminalBackground,
    linenos: bool,
}

impl IrcFormatter {
    /// Construct an [`IrcFormatter`] for a light background without line numbers.
    #[must_use]
    pub const fn new() -> Self {
        Self { bg: TerminalBackground::Light, linenos: false }
    }

    /// Select light or dark IRC palette entries.
    #[must_use]
    pub const fn with_bg(mut self, bg: TerminalBackground) -> Self {
        self.bg = bg;
        self
    }

    /// Prefix every line with a 4-digit zero-padded line number.
    #[must_use]
    pub const fn with_linenos(mut self) -> Self {
        self.linenos = true;
        self
    }
}

impl Formatter for IrcFormatter {
    fn name(&self) -> &'static str {
        "IRC"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["irc", "IRC"]
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
            if self.linenos {
                *lineno += 1;
                write!(out, "{:04}: ", *lineno)?;
            }
            Ok(())
        };
        write_lineno(out, &mut lineno)?;
        for (ty, val) in tokens {
            let pair = irc_scheme(ty);
            let color = match self.bg {
                TerminalBackground::Light => pair.0,
                TerminalBackground::Dark => pair.1,
            };
            let mut parts = val.split('\n').peekable();
            while let Some(part) = parts.next() {
                if parts.peek().is_some() {
                    if !part.is_empty() {
                        write_irc_formatted(out, color, part)?;
                    }
                    out.write_char('\n')?;
                    write_lineno(out, &mut lineno)?;
                } else if !part.is_empty() {
                    write_irc_formatted(out, color, part)?;
                }
            }
        }
        Ok(())
    }
}

// -- GroffFormatter --------------------------------------------------------------

/// Format tokens with `groff` (`troff` / `roff`) escapes.
#[derive(Clone, Debug)]
pub struct GroffFormatter {
    style: &'static Style,
    monospaced: bool,
    linenos: bool,
    wrap: usize,
}

impl Default for GroffFormatter {
    fn default() -> Self {
        Self::new(default_style())
    }
}

impl GroffFormatter {
    /// Construct a [`GroffFormatter`] using `style` (`monospaced = true`).
    #[must_use]
    pub const fn new(style: &'static Style) -> Self {
        Self { style, monospaced: true, linenos: false, wrap: 0 }
    }

    /// Use proportional serif/roman fonts instead of `CR`/`CB`/`CI`.
    #[must_use]
    pub const fn proportional(mut self) -> Self {
        self.monospaced = false;
        self
    }

    /// Enable line numbering.
    #[must_use]
    pub const fn with_linenos(mut self) -> Self {
        self.linenos = true;
        self
    }

    /// Wrap lines after `width` characters (`0` disables wrapping).
    #[must_use]
    pub const fn with_wrap(mut self, width: usize) -> Self {
        self.wrap = width;
        self
    }

    fn tags_for(&self, ty: TokenType) -> (String, String) {
        let st = self.style.style_for(ty);
        let (regular, bold, italic) =
            if self.monospaced { ("\\f[CR]", "\\f[CB]", "\\f[CI]") } else { ("\\f[R]", "\\f[B]", "\\f[I]") };
        let mut start = String::new();
        let mut end = String::new();
        if let Some(c) = st.color {
            start.push_str("\\m[");
            start.push_str(&c.to_hex());
            start.push(']');
            end.insert_str(0, "\\m[]");
        }
        if st.bold {
            start.push_str(bold);
            end.insert_str(0, regular);
        }
        if st.italic {
            start.push_str(italic);
            end.insert_str(0, regular);
        }
        if let Some(bg) = st.background {
            start.push_str("\\M[");
            start.push_str(&bg.to_hex());
            start.push(']');
            end.insert_str(0, "\\M[]");
        }
        (start, end)
    }
}

fn escape_groff(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str(r"\[u005C]"),
            '.' => out.push_str(r"\[char46]"),
            '\'' => out.push_str(r"\[u0027]"),
            '`' => out.push_str(r"\[u0060]"),
            '~' => out.push_str(r"\[u007E]"),
            c if !c.is_ascii() => {
                use fmt::Write as _;
                let _ = write!(out, r"\[u{:04X}]", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

fn wrap_groff_line(line: &str, wrap: usize, linenos: bool, linelen: &mut usize) -> String {
    let content = line.strip_suffix('\n').unwrap_or(line);
    let len = content.chars().count();
    let space = if linenos { "     " } else { "" };
    if len > wrap {
        let mut out = String::new();
        let chars: Vec<char> = content.chars().collect();
        let full = len / wrap;
        for i in 0..full {
            for &c in &chars[i * wrap..(i + 1) * wrap] {
                out.push(c);
            }
            out.push('\n');
            out.push_str(space);
        }
        let rem = len % wrap;
        if rem > 0 {
            for &c in &chars[chars.len() - rem..] {
                out.push(c);
            }
            if line.ends_with('\n') {
                out.push('\n');
            }
            *linelen = rem;
        }
        out
    } else if *linelen + len > wrap {
        *linelen = len;
        format!("\n{space}{line}")
    } else {
        *linelen += len;
        line.to_owned()
    }
}

impl Formatter for GroffFormatter {
    fn name(&self) -> &'static str {
        "groff"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["groff", "troff", "roff"]
    }

    fn filenames(&self) -> &'static [&'static str] {
        &[]
    }

    fn format_tokens(
        &self,
        tokens: &mut dyn Iterator<Item = (TokenType, &str)>,
        out: &mut dyn fmt::Write,
    ) -> fmt::Result {
        let mut colors = BTreeSet::new();
        for (_, st) in self.style.iter() {
            if let Some(c) = st.color {
                colors.insert(c.to_hex());
            }
        }
        for hex in &colors {
            writeln!(out, ".defcolor {hex} rgb #{hex}")?;
        }
        out.write_str(".nf\n\\f[CR]\n")?;

        let mut lineno = 0usize;
        let write_lineno = |out: &mut dyn fmt::Write, lineno: &mut usize| -> fmt::Result {
            *lineno += 1;
            if *lineno != 1 {
                out.write_char('\n')?;
            }
            write!(out, "{:>4} ", *lineno)
        };
        if self.linenos {
            write_lineno(out, &mut lineno)?;
        }

        let mut linelen = 0usize;
        for (ty, val) in tokens {
            let resolved = resolve_token_style(self.style, ty);
            let (start, end) = self.tags_for(resolved);
            for raw_line in val.split_inclusive('\n') {
                let line = if self.wrap > 0 {
                    std::borrow::Cow::Owned(wrap_groff_line(raw_line, self.wrap, self.linenos, &mut linelen))
                } else {
                    std::borrow::Cow::Borrowed(raw_line)
                };
                let stripped = line.strip_suffix('\n').unwrap_or(&line);
                let esc = escape_groff(stripped);
                if !start.is_empty() && !end.is_empty() {
                    if !esc.is_empty() {
                        write!(out, "{start}{esc}{end}")?;
                    }
                } else {
                    out.write_str(&esc)?;
                }
                if line.ends_with('\n') {
                    if self.linenos {
                        write_lineno(out, &mut lineno)?;
                    } else {
                        out.write_char('\n')?;
                    }
                    linelen = 0;
                }
            }
        }
        out.write_str("\n.fi")
    }
}
