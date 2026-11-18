//! HTML formatter, ported from `pygments.formatters.html`.

use std::collections::{BTreeSet, HashMap};
use std::fmt;

use crate::style::{Color, Style, default_style};
use crate::token::TokenType;

use super::{Formatter, escape_html};

/// How [`HtmlFormatter`] renders line numbers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HtmlLineNumbers {
    /// Do not render line numbers.
    #[default]
    None,
    /// Two-column HTML `<table>` layout (`linenos=1` / `table` in Pygments).
    Table,
    /// Inline `<span class="linenos">` prefix on each line (`linenos="inline"`).
    Inline,
}

fn webify(c: Color) -> String {
    let hex = format!("{:02X}{:02X}{:02X}", c.r, c.g, c.b);
    let b = hex.as_bytes();
    if b[0] == b[1] && b[2] == b[3] && b[4] == b[5] {
        format!("#{}{}{}", b[0] as char, b[2] as char, b[4] as char)
    } else {
        format!("#{hex}")
    }
}

fn ttype_class(ty: TokenType) -> String {
    if ty.is_standard() {
        return ty.css_class().to_owned();
    }
    let mut suffix = String::new();
    let mut cur = ty;
    while !cur.is_standard() {
        let last = cur.name().rsplit('.').next().unwrap_or("");
        suffix = format!("-{last}{suffix}");
        cur = cur.parent().unwrap_or(TokenType::TOKEN);
    }
    format!("{}{suffix}", cur.css_class())
}

/// Format tokens as HTML `<span>` tags, optionally wrapped in `<pre>`, `<div>`,
/// line numbers, and a complete HTML document.
#[derive(Clone, Debug)]
pub struct HtmlFormatter {
    style: &'static Style,
    title: String,
    full: bool,
    nowrap: bool,
    noclasses: bool,
    classprefix: String,
    cssclass: String,
    cssstyles: String,
    prestyles: String,
    filename: String,
    wrapcode: bool,
    debug_token_types: bool,
    linenos: HtmlLineNumbers,
    linenostart: usize,
    linenostep: usize,
    linenospecial: usize,
    nobackground: bool,
    lineseparator: String,
    lineanchors: String,
    linespans: String,
    anchorlinenos: bool,
    hl_lines: BTreeSet<usize>,
    ttype2class: HashMap<TokenType, String>,
    class2style: HashMap<String, (String, TokenType, usize)>,
}

impl Default for HtmlFormatter {
    fn default() -> Self {
        Self::new(default_style())
    }
}

impl HtmlFormatter {
    /// Construct an [`HtmlFormatter`] using `style`.
    #[must_use]
    pub fn new(style: &'static Style) -> Self {
        let mut f = Self {
            style,
            title: String::new(),
            full: false,
            nowrap: false,
            noclasses: false,
            classprefix: String::new(),
            cssclass: "highlight".into(),
            cssstyles: String::new(),
            prestyles: String::new(),
            filename: String::new(),
            wrapcode: false,
            debug_token_types: false,
            linenos: HtmlLineNumbers::None,
            linenostart: 1,
            linenostep: 1,
            linenospecial: 0,
            nobackground: false,
            lineseparator: "\n".into(),
            lineanchors: String::new(),
            linespans: String::new(),
            anchorlinenos: false,
            hl_lines: BTreeSet::new(),
            ttype2class: HashMap::new(),
            class2style: HashMap::new(),
        };
        f.rebuild_stylesheet();
        f
    }

    /// Output a full standalone HTML document including `<style>` definitions.
    #[must_use]
    pub const fn with_full(mut self) -> Self {
        self.full = true;
        self
    }

    /// Document `<title>` and `<h2>` heading when `full` is enabled.
    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Omit wrapping `<div>`/`<pre>` elements and emit only token `<span>`s.
    #[must_use]
    pub const fn with_nowrap(mut self) -> Self {
        self.nowrap = true;
        self
    }

    /// Inline CSS `style="..."` attributes instead of `class="..."` attributes.
    #[must_use]
    pub const fn without_classes(mut self) -> Self {
        self.noclasses = true;
        self
    }

    /// Prefix prepended to all CSS token classes.
    #[must_use]
    pub fn with_classprefix(mut self, prefix: impl Into<String>) -> Self {
        self.classprefix = prefix.into();
        self.rebuild_stylesheet();
        self
    }

    /// CSS class for the wrapping `<div>` (default `"highlight"`).
    #[must_use]
    pub fn with_cssclass(mut self, cls: impl Into<String>) -> Self {
        self.cssclass = cls.into();
        self
    }

    /// Inline CSS properties added to the wrapping `<div>`.
    #[must_use]
    pub fn with_cssstyles(mut self, styles: impl Into<String>) -> Self {
        self.cssstyles = styles.into();
        self
    }

    /// Inline CSS properties added to the wrapping `<pre>`.
    #[must_use]
    pub fn with_prestyles(mut self, styles: impl Into<String>) -> Self {
        self.prestyles = styles.into();
        self
    }

    /// Filename banner shown above the highlighted block.
    #[must_use]
    pub fn with_filename(mut self, filename: impl Into<String>) -> Self {
        self.filename = filename.into();
        self
    }

    /// Wrap code inside `<pre><code>...</code></pre>`.
    #[must_use]
    pub const fn with_wrapcode(mut self) -> Self {
        self.wrapcode = true;
        self
    }

    /// Add `title="Token.Type"` attributes on every token `<span>`.
    #[must_use]
    pub const fn with_debug_token_types(mut self) -> Self {
        self.debug_token_types = true;
        self
    }

    /// Line numbering mode (default [`HtmlLineNumbers::None`]).
    #[must_use]
    pub const fn with_linenos(mut self, mode: HtmlLineNumbers) -> Self {
        self.linenos = mode;
        self
    }

    /// Line number for the first line (default `1`).
    #[must_use]
    pub const fn with_linenostart(mut self, start: usize) -> Self {
        self.linenostart = start;
        self
    }

    /// Print every `step`th line number (default `1`).
    #[must_use]
    pub const fn with_linenostep(mut self, step: usize) -> Self {
        self.linenostep = if step == 0 { 1 } else { step };
        self
    }

    /// Highlight every `special`th line number with `.special` (`0` disables).
    #[must_use]
    pub const fn with_linenospecial(mut self, special: usize) -> Self {
        self.linenospecial = special;
        self
    }

    /// Suppress style background color in CSS and inline styles.
    #[must_use]
    pub const fn without_background(mut self) -> Self {
        self.nobackground = true;
        self
    }

    /// Separator emitted between output lines (default `"\n"`).
    #[must_use]
    pub fn with_lineseparator(mut self, sep: impl Into<String>) -> Self {
        self.lineseparator = sep.into();
        self
    }

    /// Prefix for `<a id="..." name="...">` anchors on every line.
    #[must_use]
    pub fn with_lineanchors(mut self, prefix: impl Into<String>) -> Self {
        self.lineanchors = prefix.into();
        self
    }

    /// Prefix for `<span id="...">` wrappers around every line.
    #[must_use]
    pub fn with_linespans(mut self, prefix: impl Into<String>) -> Self {
        self.linespans = prefix.into();
        self
    }

    /// Wrap line numbers in `<a href="#prefix-N">` links.
    #[must_use]
    pub const fn with_anchorlinenos(mut self) -> Self {
        self.anchorlinenos = true;
        self
    }

    /// 1-based line numbers to highlight with `<span class="hll">`.
    #[must_use]
    pub fn with_hl_lines(mut self, lines: impl IntoIterator<Item = usize>) -> Self {
        self.hl_lines = lines.into_iter().collect();
        self
    }

    fn css_class(&self, ty: TokenType) -> String {
        let cls = ttype_class(ty);
        if cls.is_empty() { String::new() } else { format!("{}{cls}", self.classprefix) }
    }

    fn css_classes(&self, ty: TokenType) -> String {
        let mut cls = self.css_class(ty);
        let mut cur = ty;
        while !cur.is_standard() {
            cur = cur.parent().unwrap_or(TokenType::TOKEN);
            let pcls = self.css_class(cur);
            cls = format!("{pcls} {cls}");
        }
        cls
    }

    fn css_inline_style(&self, ty: TokenType) -> Option<&str> {
        for anc in ty.ancestors() {
            if let Some(cls) = self.ttype2class.get(&anc) {
                if cls.is_empty() {
                    return None;
                }
                return self.class2style.get(cls).map(|(s, _, _)| s.as_str());
            }
        }
        None
    }

    fn rebuild_stylesheet(&mut self) {
        self.ttype2class.clear();
        self.class2style.clear();
        self.ttype2class.insert(TokenType::TOKEN, String::new());
        for (ty, st) in self.style.iter() {
            let name = self.css_class(ty);
            let mut css = String::new();
            if let Some(c) = st.color {
                css.push_str("color: ");
                css.push_str(&webify(c));
                css.push_str("; ");
            }
            if st.bold {
                css.push_str("font-weight: bold; ");
            }
            if st.italic {
                css.push_str("font-style: italic; ");
            }
            if st.underline {
                css.push_str("text-decoration: underline; ");
            }
            if let Some(bg) = st.background {
                css.push_str("background-color: ");
                css.push_str(&webify(bg));
                css.push_str("; ");
            }
            if let Some(border) = st.border {
                css.push_str("border: 1px solid ");
                css.push_str(&webify(border));
                css.push_str("; ");
            }
            if !css.is_empty() {
                css.truncate(css.len() - 2);
                self.ttype2class.insert(ty, name.clone());
                let level = ty.ancestors().count().saturating_sub(1);
                self.class2style.insert(name, (css, ty, level));
            }
        }
    }

    fn linenos_style(&self) -> String {
        let color = self.style.line_number_color.map_or_else(|| "inherit".into(), |c| c.to_string());
        let bg = self.style.line_number_background.map_or_else(|| "transparent".into(), |c| c.to_string());
        format!("color: {color}; background-color: {bg}; padding-left: 5px; padding-right: 5px;")
    }

    fn linenos_special_style(&self) -> String {
        let color = self.style.line_number_special_color.map_or_else(|| "#000000".into(), |c| c.to_string());
        let bg = self.style.line_number_special_background.map_or_else(|| "#ffffc0".into(), |c| c.to_string());
        format!("color: {color}; background-color: {bg}; padding-left: 5px; padding-right: 5px;")
    }

    fn css_prefix(arg: &str, cls: &str) -> String {
        let cls_dot = if cls.is_empty() { String::new() } else { format!(".{cls}") };
        if arg.is_empty() { cls_dot } else { format!("{arg} {cls_dot}") }
    }

    fn format_lines(&self, tokens: &mut dyn Iterator<Item = (TokenType, &str)>) -> Vec<(u8, String)> {
        let mut openers: HashMap<TokenType, String> = HashMap::new();
        let mut result = Vec::new();
        let mut lspan = String::new();
        let mut line = String::new();
        let lsep = &self.lineseparator;

        for (ty, val) in tokens {
            let cspan = openers.entry(ty).or_insert_with(|| {
                let title =
                    if self.debug_token_types { format!(" title=\"{}\"", ty.short_path()) } else { String::new() };
                if self.noclasses {
                    match self.css_inline_style(ty) {
                        Some(css) => format!("<span style=\"{css}\"{title}>"),
                        None => String::new(),
                    }
                } else {
                    let cls = self.css_classes(ty);
                    if cls.is_empty() { String::new() } else { format!("<span class=\"{cls}\"{title}>") }
                }
            });
            let cspan = cspan.as_str();
            let mut escaped = String::new();
            escape_html(val, &mut escaped);
            let mut parts: Vec<&str> = escaped.split('\n').collect();
            let last_part = parts.pop().unwrap_or("");
            for part in parts {
                if !line.is_empty() {
                    if lspan != cspan && !part.is_empty() {
                        if !lspan.is_empty() {
                            line.push_str("</span>");
                        }
                        line.push_str(cspan);
                        line.push_str(part);
                        if !cspan.is_empty() {
                            line.push_str("</span>");
                        }
                        line.push_str(lsep);
                    } else {
                        line.push_str(part);
                        if !lspan.is_empty() {
                            line.push_str("</span>");
                        }
                        line.push_str(lsep);
                    }
                    result.push((1, std::mem::take(&mut line)));
                } else if !part.is_empty() {
                    let close = if cspan.is_empty() { "" } else { "</span>" };
                    result.push((1, format!("{cspan}{part}{close}{lsep}")));
                } else {
                    result.push((1, lsep.clone()));
                }
            }
            if !line.is_empty() && !last_part.is_empty() {
                if lspan != cspan {
                    if !lspan.is_empty() {
                        line.push_str("</span>");
                    }
                    line.push_str(cspan);
                    line.push_str(last_part);
                    cspan.clone_into(&mut lspan);
                } else {
                    line.push_str(last_part);
                }
            } else if !last_part.is_empty() {
                line.push_str(cspan);
                line.push_str(last_part);
                cspan.clone_into(&mut lspan);
            }
        }
        if !line.is_empty() {
            if !lspan.is_empty() {
                line.push_str("</span>");
            }
            line.push_str(lsep);
            result.push((1, line));
        }
        result
    }
}

impl Formatter for HtmlFormatter {
    fn name(&self) -> &'static str {
        "HTML"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["html"]
    }

    fn filenames(&self) -> &'static [&'static str] {
        &["*.html", "*.htm"]
    }

    fn style_defs(&self, arg: &str) -> String {
        let mut lines = vec![
            "pre { line-height: 125%; }".to_owned(),
            format!("td.linenos .normal {{ {} }}", self.linenos_style()),
            format!("span.linenos {{ {} }}", self.linenos_style()),
            format!("td.linenos .special {{ {} }}", self.linenos_special_style()),
            format!("span.linenos.special {{ {} }}", self.linenos_special_style()),
        ];

        if let Some(hl) = self.style.highlight {
            lines.push(format!("{} {{ background-color: {hl} }}", Self::css_prefix(arg, "hll")));
        }
        if !arg.is_empty()
            && !self.nobackground
            && let Some(bg) = self.style.background
        {
            let text_style = self
                .ttype2class
                .get(&TokenType::TEXT)
                .and_then(|cls| self.class2style.get(cls))
                .map_or_else(String::new, |(s, _, _)| format!(" {s}"));
            let bg_line = format!("{}{{ background: {bg};{text_style} }}", Self::css_prefix(arg, ""));
            lines.insert(5, bg_line);
        }

        let mut styles: Vec<(usize, Vec<&str>, &str, &str)> = self
            .class2style
            .iter()
            .filter(|(cls, (st, _, _))| !cls.is_empty() && !st.is_empty())
            .map(|(cls, (st, ty, level))| {
                let path = if *ty == TokenType::TOKEN { Vec::new() } else { ty.short_path().split('.').collect() };
                (*level, path, cls.as_str(), st.as_str())
            })
            .collect();
        styles.sort();
        for (_, path, cls, st) in styles {
            lines.push(format!("{} {{ {st} }} /* {} */", Self::css_prefix(arg, cls), path.join(".")));
        }
        lines.join("\n")
    }

    fn format_tokens(
        &self,
        tokens: &mut dyn Iterator<Item = (TokenType, &str)>,
        out: &mut dyn fmt::Write,
    ) -> fmt::Result {
        let mut source = self.format_lines(tokens);

        if !self.nowrap && self.linenos == HtmlLineNumbers::Inline {
            let count = source.len();
            let num_start = self.linenostart;
            let mw = (count + num_start.saturating_sub(1)).to_string().len();
            let anchor_name = if self.lineanchors.is_empty() { &self.linespans } else { &self.lineanchors };
            let st_normal = self.linenos_style();
            let st_special = self.linenos_special_style();
            for (idx, (t, inner_line)) in source.iter_mut().enumerate() {
                let num = num_start + idx;
                let print_line = num % self.linenostep == 0;
                let special = self.linenospecial != 0 && num % self.linenospecial == 0;
                let line = if print_line { format!("{num:>mw$}") } else { " ".repeat(mw) };
                let attr = match (self.noclasses, special) {
                    (true, true) => format!(" style=\"{st_special}\""),
                    (true, false) => format!(" style=\"{st_normal}\""),
                    (false, true) => " class=\"linenos special\"".into(),
                    (false, false) => " class=\"linenos\"".into(),
                };
                let span = format!("<span{attr}>{line}</span>");
                let prefix =
                    if self.anchorlinenos { format!("<a href=\"#{anchor_name}-{num}\">{span}</a>") } else { span };
                *t = 1;
                *inner_line = format!("{prefix}{inner_line}");
            }
        }

        if !self.hl_lines.is_empty() {
            for (i, (t, val)) in source.iter_mut().enumerate() {
                if *t == 1 && self.hl_lines.contains(&(i + 1)) {
                    *val = if self.noclasses {
                        let st = self
                            .style
                            .highlight
                            .map_or_else(String::new, |c| format!(" style=\"background-color: {c}\""));
                        format!("<span{st}>{val}</span>")
                    } else {
                        format!("<span class=\"hll\">{val}</span>")
                    };
                }
            }
        }

        if self.nowrap {
            for (_, piece) in &source {
                out.write_str(piece)?;
            }
            return Ok(());
        }

        if !self.lineanchors.is_empty() {
            let mut i = self.linenostart.saturating_sub(1);
            for (t, val) in &mut source {
                if *t != 0 {
                    i += 1;
                    let href = if self.linenos == HtmlLineNumbers::None {
                        format!(" href=\"#{}-{i}\"", self.lineanchors)
                    } else {
                        String::new()
                    };
                    *val =
                        format!("<a id=\"{}-{i}\" name=\"{}-{i}\"{href}></a>{val}", self.lineanchors, self.lineanchors);
                }
            }
        }
        if !self.linespans.is_empty() {
            let mut i = self.linenostart.saturating_sub(1);
            for (t, val) in &mut source {
                if *t != 0 {
                    i += 1;
                    *val = format!("<span id=\"{}-{i}\">{val}</span>", self.linespans);
                }
            }
        }

        let mut wrapped = Vec::new();
        let mut pre_styles = Vec::new();
        if !self.prestyles.is_empty() {
            pre_styles.push(self.prestyles.as_str());
        }
        if self.noclasses {
            pre_styles.push("line-height: 125%;");
        }
        if !self.filename.is_empty() && self.linenos != HtmlLineNumbers::Table {
            wrapped.push((0u8, format!("<span class=\"filename\">{}</span>", self.filename)));
        }
        let pre_open = if pre_styles.is_empty() {
            "<pre><span></span>".into()
        } else {
            format!("<pre style=\"{}\"><span></span>", pre_styles.join("; "))
        };
        wrapped.push((0, pre_open));
        if self.wrapcode {
            wrapped.push((0, "<code>".into()));
        }
        let mut lncount = 0usize;
        for (t, s) in source {
            if t != 0 {
                lncount += 1;
            }
            wrapped.push((t, s));
        }
        if self.wrapcode {
            wrapped.push((0, "</code>".into()));
        }
        wrapped.push((0, "</pre>".into()));

        if self.linenos == HtmlLineNumbers::Table {
            let fl = self.linenostart;
            let mw = (lncount + fl.saturating_sub(1)).to_string().len();
            let anchor_name = if self.lineanchors.is_empty() { &self.linespans } else { &self.lineanchors };
            let st_normal = self.linenos_style();
            let st_special = self.linenos_special_style();
            let mut lns = Vec::with_capacity(lncount);
            for i in fl..fl + lncount {
                let mut line = if i % self.linenostep == 0 {
                    let s = format!("{i:>mw$}");
                    if self.anchorlinenos { format!("<a href=\"#{anchor_name}-{i}\">{s}</a>") } else { s }
                } else {
                    " ".repeat(mw)
                };
                let special = self.linenospecial != 0 && i % self.linenospecial == 0;
                let attr = match (self.noclasses, special) {
                    (true, true) => format!(" style=\"{st_special}\""),
                    (true, false) => format!(" style=\"{st_normal}\""),
                    (false, true) => " class=\"special\"".into(),
                    (false, false) => " class=\"normal\"".into(),
                };
                line = format!("<span{attr}>{line}</span>");
                lns.push(line);
            }
            let filename_tr = if self.filename.is_empty() {
                String::new()
            } else {
                format!(
                    "<tr><th colspan=\"2\" class=\"filename\"><span class=\"filename\">{}</span></th></tr>",
                    self.filename
                )
            };
            let mut code_body = String::new();
            for (_, piece) in wrapped {
                code_body.push_str(&piece);
            }
            wrapped = vec![
                (
                    0,
                    format!(
                        "<table class=\"{}table\">{filename_tr}<tr><td class=\"linenos\"><div class=\"linenodiv\"><pre>{}</pre></div></td><td class=\"code\">",
                        self.cssclass,
                        lns.join("\n")
                    ),
                ),
                (0, "<div>".into()),
                (0, code_body),
                (0, "</div>".into()),
                (0, "</td></tr></table>".into()),
            ];
        }

        let mut div_styles = Vec::new();
        if self.noclasses
            && !self.nobackground
            && let Some(bg) = self.style.background
        {
            div_styles.push(format!("background: {bg}"));
        }
        if !self.cssstyles.is_empty() {
            div_styles.push(self.cssstyles.clone());
        }
        let div_cls = if self.cssclass.is_empty() { String::new() } else { format!(" class=\"{}\"", self.cssclass) };
        let div_st =
            if div_styles.is_empty() { String::new() } else { format!(" style=\"{}\"", div_styles.join("; ")) };
        let mut inner_out = format!("<div{div_cls}{div_st}>");
        for (_, piece) in wrapped {
            inner_out.push_str(&piece);
        }
        inner_out.push_str("</div>\n");

        if self.full {
            let cssdefs = self.style_defs("body");
            writeln!(
                out,
                "<!DOCTYPE html PUBLIC \"-//W3C//DTD HTML 4.01//EN\"\n   \"http://www.w3.org/TR/html4/strict.dtd\">\n<!--\ngenerated by Pygments <https://pygments.org/>\nCopyright 2006-2025 by the Pygments team.\nLicensed under the BSD license, see LICENSE for details.\n-->\n<html>\n<head>\n  <title>{}</title>\n  <meta http-equiv=\"content-type\" content=\"text/html; charset=utf-8\">\n  <style type=\"text/css\">\n/*\ngenerated by Pygments <https://pygments.org/>\nCopyright 2006-2025 by the Pygments team.\nLicensed under the BSD license, see LICENSE for details.\n*/\n{cssdefs}\n\n  </style>\n</head>\n<body>\n<h2>{}</h2>\n",
                self.title, self.title
            )?;
            out.write_str(&inner_out)?;
            out.write_str("</body>\n</html>\n")
        } else {
            out.write_str(&inner_out)
        }
    }
}
