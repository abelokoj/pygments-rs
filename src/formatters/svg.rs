//! SVG formatter, ported from `pygments.formatters.svg`.

use std::fmt;

use crate::style::{Style, default_style};
use crate::token::TokenType;

use super::{Formatter, escape_html, resolve_token_style};

/// Format tokens as an SVG graphic with `<text>` lines and `<tspan>` spans.
#[derive(Clone, Debug)]
pub struct SvgFormatter {
    style: &'static Style,
    nowrap: bool,
    fontfamily: String,
    fontsize: String,
    xoffset: isize,
    yoffset: Option<isize>,
    ystep: Option<isize>,
    spacehack: bool,
    linenos: bool,
    linenostart: usize,
    linenostep: usize,
    linenowidth: Option<isize>,
}

impl Default for SvgFormatter {
    fn default() -> Self {
        Self::new(default_style())
    }
}

impl SvgFormatter {
    /// Construct an [`SvgFormatter`] using `style`.
    #[must_use]
    pub fn new(style: &'static Style) -> Self {
        Self {
            style,
            nowrap: false,
            fontfamily: "monospace".into(),
            fontsize: "14px".into(),
            xoffset: 0,
            yoffset: None,
            ystep: None,
            spacehack: true,
            linenos: false,
            linenostart: 1,
            linenostep: 1,
            linenowidth: None,
        }
    }

    /// Omit XML/SVG wrapper tags, emitting only `<text>` lines.
    #[must_use]
    pub const fn with_nowrap(mut self) -> Self {
        self.nowrap = true;
        self
    }

    /// Set the `<g font-family="...">` value (default `"monospace"`).
    #[must_use]
    pub fn with_fontfamily(mut self, family: impl Into<String>) -> Self {
        self.fontfamily = family.into();
        self
    }

    /// Set the `<g font-size="...">` value (default `"14px"`).
    #[must_use]
    pub fn with_fontsize(mut self, size: impl Into<String>) -> Self {
        self.fontsize = size.into();
        self
    }

    /// Starting X coordinate (default `0`).
    #[must_use]
    pub const fn with_xoffset(mut self, x: isize) -> Self {
        self.xoffset = x;
        self
    }

    /// Starting Y baseline coordinate (defaults to numeric font size or `20`).
    #[must_use]
    pub const fn with_yoffset(mut self, y: isize) -> Self {
        self.yoffset = Some(y);
        self
    }

    /// Vertical line step (defaults to numeric font size + `5`, or `25`).
    #[must_use]
    pub const fn with_ystep(mut self, step: isize) -> Self {
        self.ystep = Some(step);
        self
    }

    /// Keep literal spaces and tabs instead of converting them to `&#160;`.
    #[must_use]
    pub const fn without_spacehack(mut self) -> Self {
        self.spacehack = false;
        self
    }

    /// Enable line numbers.
    #[must_use]
    pub const fn with_linenos(mut self) -> Self {
        self.linenos = true;
        self
    }

    /// First line number (default `1`).
    #[must_use]
    pub const fn with_linenostart(mut self, start: usize) -> Self {
        self.linenostart = start;
        self
    }

    /// Print every `step`th line number (default `1`).
    #[must_use]
    pub const fn with_linenostep(mut self, step: usize) -> Self {
        self.linenostep = step;
        self
    }

    /// Width reserved for line numbers (defaults to `3 * ystep`).
    #[must_use]
    pub const fn with_linenowidth(mut self, width: isize) -> Self {
        self.linenowidth = Some(width);
        self
    }

    fn parsed_fontsize(&self) -> isize {
        let fs = self.fontsize.trim();
        let fs = fs.strip_suffix("px").map_or(fs, str::trim);
        fs.parse().unwrap_or(20)
    }

    fn tspan_attrs(&self, ty: TokenType) -> String {
        let resolved = resolve_token_style(self.style, ty);
        let st = self.style.style_for(resolved);
        let mut attrs = String::new();
        if let Some(c) = st.color {
            attrs.push_str(" fill=\"#");
            attrs.push_str(&c.to_hex());
            attrs.push('"');
        }
        if st.bold {
            attrs.push_str(" font-weight=\"bold\"");
        }
        if st.italic {
            attrs.push_str(" font-style=\"italic\"");
        }
        attrs
    }
}

fn expandtabs_8(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut col = 0usize;
    for ch in s.chars() {
        match ch {
            '\t' => {
                let n = 8 - (col % 8);
                for _ in 0..n {
                    out.push(' ');
                }
                col += n;
            }
            '\n' | '\r' => {
                out.push(ch);
                col = 0;
            }
            c => {
                out.push(c);
                col += 1;
            }
        }
    }
    out
}

impl Formatter for SvgFormatter {
    fn name(&self) -> &'static str {
        "SVG"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["svg"]
    }

    fn filenames(&self) -> &'static [&'static str] {
        &["*.svg"]
    }

    fn format_tokens(
        &self,
        tokens: &mut dyn Iterator<Item = (TokenType, &str)>,
        out: &mut dyn fmt::Write,
    ) -> fmt::Result {
        let int_fs = self.parsed_fontsize();
        let x = self.xoffset;
        let mut y = self.yoffset.unwrap_or(int_fs);
        let ystep = self.ystep.unwrap_or(int_fs + 5);
        let linenowidth = self.linenowidth.unwrap_or(3 * ystep);

        if !self.nowrap {
            out.write_str("<?xml version=\"1.0\"?>\n")?;
            out.write_str(
                "<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.0//EN\" \"http://www.w3.org/TR/2001/REC-SVG-20010904/DTD/svg10.dtd\">\n",
            )?;
            out.write_str("<svg xmlns=\"http://www.w3.org/2000/svg\">\n")?;
            writeln!(out, "<g font-family=\"{}\" font-size=\"{}\">", self.fontfamily, self.fontsize)?;
        }

        let mut counter = self.linenostart;
        let counter_step = self.linenostep.max(1);
        let counter_style = self.tspan_attrs(TokenType::COMMENT);
        let mut line_x = x;

        if self.linenos {
            if counter % counter_step == 0 {
                write!(
                    out,
                    "<text x=\"{}\" y=\"{y}\" {counter_style} text-anchor=\"end\">{counter}</text>",
                    x + linenowidth
                )?;
            }
            line_x += linenowidth + ystep;
            counter += 1;
        }

        write!(out, "<text x=\"{line_x}\" y=\"{y}\" xml:space=\"preserve\">")?;
        for (ty, val) in tokens {
            let attrs = self.tspan_attrs(ty);
            let (open, close) =
                if attrs.is_empty() { (String::new(), "") } else { (format!("<tspan{attrs}>"), "</tspan>") };
            let mut escaped = String::new();
            escape_html(val, &mut escaped);
            let rendered = if self.spacehack { expandtabs_8(&escaped).replace(' ', "&#160;") } else { escaped };
            let mut parts = rendered.split('\n').peekable();
            while let Some(part) = parts.next() {
                if parts.peek().is_some() {
                    writeln!(out, "{open}{part}{close}</text>")?;
                    y += ystep;
                    if self.linenos && counter % counter_step == 0 {
                        write!(
                            out,
                            "<text x=\"{}\" y=\"{y}\" text-anchor=\"end\" {counter_style}>{counter}</text>",
                            x + linenowidth
                        )?;
                    }
                    counter += 1;
                    write!(out, "<text x=\"{line_x}\" y=\"{y}\" xml:space=\"preserve\">")?;
                } else {
                    write!(out, "{open}{part}{close}")?;
                }
            }
        }
        out.write_str("</text>")?;
        if !self.nowrap {
            out.write_str("</g></svg>\n")?;
        }
        Ok(())
    }
}
