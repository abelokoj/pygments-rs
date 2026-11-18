//! Output formatters, ported from `pygments.formatters`.
//!
//! Includes all 14 pure-text Pygments formatters (`html`, `svg`, `latex`, `rtf`,
//! `terminal`, `terminal256`, `terminal16m`, `bbcode`, `pango`, `irc`, `groff`,
//! `raw`, `text`/`null`, and `testcase`).

use std::fmt;

use crate::lexer::Lexer;
use crate::registry::fnmatch;
use crate::style::{Color, Style, style_by_name};
use crate::token::TokenType;

pub mod html;
pub mod latex;
pub mod other;
pub mod rtf;
pub mod svg;
pub mod terminal;

pub use html::{HtmlFormatter, HtmlLineNumbers};
pub use latex::LatexFormatter;
pub use other::{
    BBCodeFormatter, GroffFormatter, IrcFormatter, NullFormatter, PangoMarkupFormatter, RawTokenFormatter,
    TestcaseFormatter,
};
pub use rtf::RtfFormatter;
pub use svg::SvgFormatter;
pub use terminal::{Terminal256Formatter, TerminalBackground, TerminalFormatter, TerminalTrueColorFormatter};

/// Output formatter turning a stream of `(TokenType, &str)` tokens into text.
pub trait Formatter: Send + Sync {
    /// Human-readable formatter name (e.g. `"HTML"`).
    fn name(&self) -> &'static str;

    /// Short aliases recognized by [`formatter_by_name`] and `pygmentize -f`.
    fn aliases(&self) -> &'static [&'static str];

    /// Filename globs associated with this formatter (e.g. `["*.html", "*.htm"]`).
    fn filenames(&self) -> &'static [&'static str];

    /// Format `tokens` and write the output to `out`.
    ///
    /// # Errors
    ///
    /// Propagates any [`fmt::Error`] returned by `out`.
    fn format_tokens(
        &self,
        tokens: &mut dyn Iterator<Item = (TokenType, &str)>,
        out: &mut dyn fmt::Write,
    ) -> fmt::Result;

    /// Style definitions required by this formatter (such as CSS classes for
    /// [`HtmlFormatter`] or `\PY` macros for [`LatexFormatter`]).
    fn style_defs(&self, _arg: &str) -> String {
        String::new()
    }
}

/// Format an iterator of tokens using `formatter`, writing into `out`.
///
/// # Errors
///
/// Propagates any [`fmt::Error`] from `out`.
pub fn format<'a, I, F, W>(tokens: I, formatter: &F, out: &mut W) -> fmt::Result
where
    I: IntoIterator<Item = (TokenType, &'a str)>,
    F: Formatter + ?Sized,
    W: fmt::Write,
{
    let mut iter = tokens.into_iter();
    formatter.format_tokens(&mut iter, out)
}

/// Format an iterator of tokens into a newly allocated [`String`].
#[must_use]
pub fn format_string<'a, I, F>(tokens: I, formatter: &F) -> String
where
    I: IntoIterator<Item = (TokenType, &'a str)>,
    F: Formatter + ?Sized,
{
    let mut out = String::new();
    let _ = format(tokens, formatter, &mut out);
    out
}

/// Preprocess `code` with `lexer`, tokenize it, and format the result using
/// `formatter` — matching `pygments.highlight(code, lexer, formatter)`.
#[must_use]
pub fn highlight(code: &str, lexer: Lexer, formatter: &dyn Formatter) -> String {
    let pre = lexer.preprocess(code);
    format_string(lexer.tokenize(&pre), formatter)
}

/// Error returned when looking up or configuring a formatter by name or
/// filename.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormatterError(pub String);

impl fmt::Display for FormatterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for FormatterError {}

static FORMATTER_NAMES: [&str; 14] = [
    "bbcode",
    "groff",
    "html",
    "irc",
    "latex",
    "null",
    "pango",
    "raw",
    "rtf",
    "svg",
    "terminal",
    "terminal16m",
    "terminal256",
    "testcase",
];

/// Primary names of all built-in pure-text formatters, sorted.
#[must_use]
pub fn formatter_names() -> &'static [&'static str] {
    &FORMATTER_NAMES
}

pub(crate) fn escape_html(text: &str, out: &mut String) {
    out.reserve(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
}

pub(crate) fn resolve_token_style(style: &'static Style, mut ty: TokenType) -> TokenType {
    while !style.styles_token(ty) {
        match ty.parent() {
            Some(p) => ty = p,
            None => break,
        }
    }
    ty
}

fn py_printable_non_ascii(c: char) -> bool {
    let n = c as u32;
    !matches!(
        n,
        0x80..=0xa0
            | 0xad
            | 0x600..=0x605
            | 0x61c
            | 0x6dd
            | 0x70f
            | 0x1680
            | 0x180e
            | 0x2000..=0x200f
            | 0x2028..=0x202f
            | 0x205f..=0x206f
            | 0x3000
            | 0xd800..=0xf8ff
            | 0xfeff
            | 0xfff9..=0xfffb
            | 0xe0001
            | 0xe0020..=0xe007f
            | 0xf0000..=0x10ffff
    )
}

pub(crate) fn py_repr(s: &str, ascii_only: bool) -> String {
    use fmt::Write as _;
    let quote = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c == '\x7f' => {
                let _ = write!(out, "\\x{:02x}", c as u32);
            }
            c if !c.is_ascii() && (ascii_only || !py_printable_non_ascii(c)) => {
                let n = c as u32;
                let _ = if n <= 0xff {
                    write!(out, "\\x{n:02x}")
                } else if n <= 0xffff {
                    write!(out, "\\u{n:04x}")
                } else {
                    write!(out, "\\U{n:08x}")
                };
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

fn opt_val<'a>(options: &[(&'a str, &'a str)], key: &str) -> Option<&'a str> {
    options.iter().rfind(|&&(k, _)| k == key).map(|&(_, v)| v)
}

fn parse_bool(s: &str) -> Result<bool, FormatterError> {
    match s.to_ascii_lowercase().as_str() {
        "1" | "yes" | "true" | "on" => Ok(true),
        "0" | "no" | "false" | "off" => Ok(false),
        _ => Err(FormatterError(format!("invalid boolean option value {s:?}"))),
    }
}

fn opt_bool(options: &[(&str, &str)], key: &str) -> Result<bool, FormatterError> {
    opt_val(options, key).map(parse_bool).transpose().map(|b| b.unwrap_or(false))
}

fn parse_usize(s: &str, name: &str) -> Result<usize, FormatterError> {
    s.parse().map_err(|_| FormatterError(format!("invalid integer for {name}: {s:?}")))
}

fn parse_isize(s: &str, name: &str) -> Result<isize, FormatterError> {
    s.parse().map_err(|_| FormatterError(format!("invalid integer for {name}: {s:?}")))
}

fn parse_style(options: &[(&str, &str)]) -> Result<&'static Style, FormatterError> {
    let name = opt_val(options, "style").unwrap_or("default");
    style_by_name(name).ok_or_else(|| FormatterError(format!("style {name:?} not found")))
}

fn parse_bg(options: &[(&str, &str)]) -> Result<TerminalBackground, FormatterError> {
    match opt_val(options, "bg") {
        None | Some("light") => Ok(TerminalBackground::Light),
        Some("dark") => Ok(TerminalBackground::Dark),
        Some(other) => Err(FormatterError(format!("invalid bg {other:?} (expected \"light\" or \"dark\")"))),
    }
}

fn parse_hl_lines(s: &str) -> Vec<usize> {
    s.split(|c: char| c.is_whitespace() || c == ',').filter_map(|p| p.parse::<usize>().ok()).collect()
}

/// Instantiate a formatter by alias (`"html"`, `"terminal"`, `"latex"`, etc.)
/// with string key-value options matching `pygmentize -O`.
///
/// # Errors
///
/// Returns [`FormatterError`] if `alias` is unknown or any option value is
/// invalid.
pub fn formatter_by_name(alias: &str, options: &[(&str, &str)]) -> Result<Box<dyn Formatter>, FormatterError> {
    let opt = |k| opt_val(options, k);
    let bopt = |k| opt_bool(options, k);
    match alias {
        "html" => {
            let mut f = HtmlFormatter::new(parse_style(options)?);
            if bopt("full")? {
                f = f.with_full();
            }
            if let Some(v) = opt("title") {
                f = f.with_title(v);
            }
            if bopt("nowrap")? {
                f = f.with_nowrap();
            }
            if bopt("noclasses")? {
                f = f.without_classes();
            }
            if let Some(v) = opt("classprefix") {
                f = f.with_classprefix(v);
            }
            if let Some(v) = opt("cssclass") {
                f = f.with_cssclass(v);
            }
            if let Some(v) = opt("cssstyles") {
                f = f.with_cssstyles(v);
            }
            if let Some(v) = opt("prestyles") {
                f = f.with_prestyles(v);
            }
            if let Some(v) = opt("filename") {
                f = f.with_filename(v);
            }
            if bopt("wrapcode")? {
                f = f.with_wrapcode();
            }
            if bopt("debug_token_types")? {
                f = f.with_debug_token_types();
            }
            if let Some(v) = opt("linenos") {
                let mode = if v.eq_ignore_ascii_case("inline") {
                    HtmlLineNumbers::Inline
                } else if v.eq_ignore_ascii_case("table") || (!v.is_empty() && parse_bool(v)?) {
                    HtmlLineNumbers::Table
                } else {
                    HtmlLineNumbers::None
                };
                f = f.with_linenos(mode);
            }
            if let Some(v) = opt("linenostart") {
                f = f.with_linenostart(parse_usize(v, "linenostart")?);
            }
            if let Some(v) = opt("linenostep") {
                f = f.with_linenostep(parse_usize(v, "linenostep")?);
            }
            if let Some(v) = opt("linenospecial") {
                f = f.with_linenospecial(parse_usize(v, "linenospecial")?);
            }
            if bopt("nobackground")? {
                f = f.without_background();
            }
            if let Some(v) = opt("lineseparator") {
                f = f.with_lineseparator(v);
            }
            if let Some(v) = opt("lineanchors") {
                f = f.with_lineanchors(v);
            }
            if let Some(v) = opt("linespans") {
                f = f.with_linespans(v);
            }
            if bopt("anchorlinenos")? {
                f = f.with_anchorlinenos();
            }
            if let Some(v) = opt("hl_lines") {
                f = f.with_hl_lines(parse_hl_lines(v));
            }
            Ok(Box::new(f))
        }
        "terminal" | "console" => {
            let mut f = TerminalFormatter::new().with_bg(parse_bg(options)?);
            if bopt("linenos")? {
                f = f.with_linenos();
            }
            Ok(Box::new(f))
        }
        "terminal256" | "console256" | "256" => {
            let mut f = Terminal256Formatter::new(parse_style(options)?);
            if opt("nobold").is_some() {
                f = f.without_bold();
            }
            if opt("nounderline").is_some() {
                f = f.without_underline();
            }
            if opt("noitalic").is_some() {
                f = f.without_italic();
            }
            if bopt("linenos")? {
                f = f.with_linenos();
            }
            Ok(Box::new(f))
        }
        "terminal16m" | "console16m" | "16m" => {
            let mut f = TerminalTrueColorFormatter::new(parse_style(options)?);
            if opt("nobold").is_some() {
                f = f.without_bold();
            }
            if opt("nounderline").is_some() {
                f = f.without_underline();
            }
            if opt("noitalic").is_some() {
                f = f.without_italic();
            }
            if bopt("linenos")? {
                f = f.with_linenos();
            }
            Ok(Box::new(f))
        }
        "latex" | "tex" => {
            let mut f = LatexFormatter::new(parse_style(options)?);
            if bopt("full")? {
                f = f.with_full();
            }
            if let Some(v) = opt("title") {
                f = f.with_title(v);
            }
            if bopt("nowrap")? {
                f = f.with_nowrap();
            }
            if let Some(v) = opt("docclass") {
                f = f.with_docclass(v);
            }
            if let Some(v) = opt("preamble") {
                f = f.with_preamble(v);
            }
            if bopt("linenos")? {
                f = f.with_linenos();
            }
            if let Some(v) = opt("linenostart") {
                f = f.with_linenostart(parse_usize(v, "linenostart")?);
            }
            if let Some(v) = opt("linenostep") {
                f = f.with_linenostep(parse_usize(v, "linenostep")?);
            }
            if let Some(v) = opt("verboptions") {
                f = f.with_verboptions(v);
            }
            if let Some(v) = opt("commandprefix") {
                f = f.with_commandprefix(v);
            }
            if bopt("texcomments")? {
                f = f.with_texcomments();
            }
            if bopt("mathescape")? {
                f = f.with_mathescape();
            }
            if let Some(v) = opt("escapeinside") {
                f = f.with_escapeinside(v);
            }
            if let Some(v) = opt("envname") {
                f = f.with_envname(v);
            }
            Ok(Box::new(f))
        }
        "rtf" => {
            let mut f = RtfFormatter::new(parse_style(options)?);
            if let Some(v) = opt("fontface") {
                f = f.with_fontface(v);
            }
            if let Some(v) = opt("fontsize") {
                f = f.with_fontsize(parse_usize(v, "fontsize")?);
            }
            if bopt("linenos")? {
                f = f.with_linenos();
            }
            if let Some(v) = opt("lineno_fontsize") {
                f = f.with_lineno_fontsize(parse_usize(v, "lineno_fontsize")?);
            }
            if let Some(v) = opt("lineno_padding") {
                f = f.with_lineno_padding(parse_usize(v, "lineno_padding")?);
            }
            if let Some(v) = opt("linenostart") {
                f = f.with_linenostart(parse_usize(v, "linenostart")?);
            }
            if let Some(v) = opt("linenostep") {
                f = f.with_linenostep(parse_usize(v, "linenostep")?);
            }
            if let Some(v) = opt("lineno_color") {
                let c = Color::parse(v).ok_or_else(|| FormatterError(format!("invalid color {v:?}")))?;
                f = f.with_lineno_color(c);
            }
            if let Some(v) = opt("hl_color") {
                let c = Color::parse(v).ok_or_else(|| FormatterError(format!("invalid color {v:?}")))?;
                f = f.with_hl_color(c);
            }
            if let Some(v) = opt("hl_lines") {
                f = f.with_hl_lines(parse_hl_lines(v));
            }
            if bopt("hl_linenostart")? {
                f = f.with_hl_linenostart();
            }
            Ok(Box::new(f))
        }
        "svg" => {
            let mut f = SvgFormatter::new(parse_style(options)?);
            if bopt("nowrap")? {
                f = f.with_nowrap();
            }
            if let Some(v) = opt("fontfamily") {
                f = f.with_fontfamily(v);
            }
            if let Some(v) = opt("fontsize") {
                f = f.with_fontsize(v);
            }
            if let Some(v) = opt("xoffset") {
                f = f.with_xoffset(parse_isize(v, "xoffset")?);
            }
            if let Some(v) = opt("yoffset") {
                f = f.with_yoffset(parse_isize(v, "yoffset")?);
            }
            if let Some(v) = opt("ystep") {
                f = f.with_ystep(parse_isize(v, "ystep")?);
            }
            if let Some(v) = opt("spacehack")
                && !parse_bool(v)?
            {
                f = f.without_spacehack();
            }
            if bopt("linenos")? {
                f = f.with_linenos();
            }
            if let Some(v) = opt("linenostart") {
                f = f.with_linenostart(parse_usize(v, "linenostart")?);
            }
            if let Some(v) = opt("linenostep") {
                f = f.with_linenostep(parse_usize(v, "linenostep")?);
            }
            if let Some(v) = opt("linenowidth") {
                f = f.with_linenowidth(parse_isize(v, "linenowidth")?);
            }
            Ok(Box::new(f))
        }
        "bbcode" | "bb" => {
            let mut f = BBCodeFormatter::new(parse_style(options)?);
            if bopt("codetag")? {
                f = f.with_codetag();
            }
            if bopt("monofont")? {
                f = f.with_monofont();
            }
            Ok(Box::new(f))
        }
        "pango" | "pangomarkup" => Ok(Box::new(PangoMarkupFormatter::new(parse_style(options)?))),
        "irc" | "IRC" => {
            let mut f = IrcFormatter::new().with_bg(parse_bg(options)?);
            if bopt("linenos")? {
                f = f.with_linenos();
            }
            Ok(Box::new(f))
        }
        "groff" | "troff" | "roff" => {
            let mut f = GroffFormatter::new(parse_style(options)?);
            if let Some(v) = opt("monospaced")
                && !parse_bool(v)?
            {
                f = f.proportional();
            }
            if bopt("linenos")? {
                f = f.with_linenos();
            }
            if let Some(v) = opt("wrap") {
                f = f.with_wrap(parse_usize(v, "wrap")?);
            }
            Ok(Box::new(f))
        }
        "raw" | "tokens" => {
            let mut f = RawTokenFormatter::new();
            if let Some(c) = opt("error_color") {
                let color = if c.eq_ignore_ascii_case("true") { "red" } else { c };
                f = f
                    .with_error_color(color)
                    .ok_or_else(|| FormatterError(format!("invalid error_color {color:?}")))?;
            }
            Ok(Box::new(f))
        }
        "text" | "null" => Ok(Box::new(NullFormatter::new())),
        "testcase" => Ok(Box::new(TestcaseFormatter::new())),
        _ => Err(FormatterError(format!("formatter {alias:?} not found"))),
    }
}

/// Look up and instantiate a formatter matching the basename of `filename`.
///
/// # Errors
///
/// Returns [`FormatterError`] if no formatter handles `filename` or an option
/// is invalid.
pub fn formatter_for_filename(filename: &str, options: &[(&str, &str)]) -> Result<Box<dyn Formatter>, FormatterError> {
    let base = filename.rsplit(['/', '\\']).next().unwrap_or(filename);
    let table: &[(&str, &[&str])] = &[
        ("html", &["*.html", "*.htm"]),
        ("latex", &["*.tex"]),
        ("rtf", &["*.rtf"]),
        ("svg", &["*.svg"]),
        ("raw", &["*.raw"]),
        ("text", &["*.txt"]),
    ];
    for &(alias, globs) in table {
        if globs.iter().any(|g| fnmatch(base, g)) {
            return formatter_by_name(alias, options);
        }
    }
    Err(FormatterError(format!("no formatter found for filename {filename:?}")))
}
