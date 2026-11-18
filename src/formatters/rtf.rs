//! RTF formatter, ported from `pygments.formatters.rtf`.

use std::collections::BTreeSet;
use std::fmt;

use crate::style::{Color, Style, default_style};
use crate::token::TokenType;

use super::{Formatter, resolve_token_style};

const ANSI_BRIGHT_BLACK: Color = Color { r: 0x55, g: 0x55, b: 0x55 };

fn escape_rtf_text(text: &str, out: &mut String) {
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            '\n' => out.push_str("\\par"),
            c if (c as u32) < 128 => out.push(c),
            c if (c as u32) < 65536 => {
                use fmt::Write as _;
                let _ = write!(out, "{{\\u{}}}", c as u32);
            }
            c => {
                use fmt::Write as _;
                let mut pair = [0u16; 2];
                for unit in c.encode_utf16(&mut pair) {
                    let _ = write!(out, "{{\\u{unit}}}");
                }
            }
        }
    }
}

fn push_color(colors: &mut Vec<Color>, color: Color) -> usize {
    if let Some(idx) = colors.iter().position(|&c| c == color) {
        idx + 1
    } else {
        colors.push(color);
        colors.len()
    }
}

/// Format tokens as Rich Text Format (`\rtf1`) markup.
#[derive(Clone, Debug)]
pub struct RtfFormatter {
    style: &'static Style,
    fontface: String,
    fontsize: usize,
    linenos: bool,
    lineno_fontsize: Option<usize>,
    lineno_padding: usize,
    linenostart: usize,
    linenostep: usize,
    hl_linenostart: bool,
    hl_color: Option<Color>,
    lineno_color: Option<Color>,
    hl_lines: BTreeSet<usize>,
}

impl Default for RtfFormatter {
    fn default() -> Self {
        Self::new(default_style())
    }
}

impl RtfFormatter {
    /// Construct an [`RtfFormatter`] using `style`.
    #[must_use]
    pub fn new(style: &'static Style) -> Self {
        Self {
            style,
            fontface: String::new(),
            fontsize: 0,
            linenos: false,
            lineno_fontsize: None,
            lineno_padding: 2,
            linenostart: 1,
            linenostep: 1,
            hl_linenostart: false,
            hl_color: None,
            lineno_color: None,
            hl_lines: BTreeSet::new(),
        }
    }

    /// Font family name added to the RTF `\fonttbl` entry.
    #[must_use]
    pub fn with_fontface(mut self, fontface: impl Into<String>) -> Self {
        self.fontface = fontface.into();
        self
    }

    /// Font size in half-points (`0` uses the reader's default).
    #[must_use]
    pub const fn with_fontsize(mut self, fontsize: usize) -> Self {
        self.fontsize = fontsize;
        self
    }

    /// Enable inline line numbering.
    #[must_use]
    pub const fn with_linenos(mut self) -> Self {
        self.linenos = true;
        self
    }

    /// Font size for line numbers in half-points.
    #[must_use]
    pub const fn with_lineno_fontsize(mut self, fontsize: usize) -> Self {
        self.lineno_fontsize = Some(fontsize);
        self
    }

    /// Spaces emitted between line numbers and source code (default `2`).
    #[must_use]
    pub const fn with_lineno_padding(mut self, padding: usize) -> Self {
        self.lineno_padding = padding;
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
        self.linenostep = if step == 0 { 1 } else { step };
        self
    }

    /// Line number color override (defaults to style's `line_number_color` or `#555555`).
    #[must_use]
    pub const fn with_lineno_color(mut self, color: Color) -> Self {
        self.lineno_color = Some(color);
        self
    }

    /// Highlight background color override (defaults to style's `highlight`).
    #[must_use]
    pub const fn with_hl_color(mut self, color: Color) -> Self {
        self.hl_color = Some(color);
        self
    }

    /// 1-based line numbers to highlight with `\highlightN`.
    #[must_use]
    pub fn with_hl_lines(mut self, lines: impl IntoIterator<Item = usize>) -> Self {
        self.hl_lines = lines.into_iter().collect();
        self
    }

    /// Treat `hl_lines` entries as relative to `linenostart` (`hl_linenostart`).
    #[must_use]
    pub const fn with_hl_linenostart(mut self) -> Self {
        self.hl_linenostart = true;
        self
    }
}

impl Formatter for RtfFormatter {
    fn name(&self) -> &'static str {
        "RTF"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["rtf"]
    }

    fn filenames(&self) -> &'static [&'static str] {
        &["*.rtf"]
    }

    fn format_tokens(
        &self,
        tokens: &mut dyn Iterator<Item = (TokenType, &str)>,
        out: &mut dyn fmt::Write,
    ) -> fmt::Result {
        let lineno_col = self.lineno_color.or(self.style.line_number_color).unwrap_or(ANSI_BRIGHT_BLACK);
        let hl_col = self.hl_color.or(self.style.highlight).unwrap_or(Color { r: 0xff, g: 0xff, b: 0xcc });

        let mut colors = Vec::new();
        let lineno_idx = self.linenos.then(|| push_color(&mut colors, lineno_col));
        let hl_lines: BTreeSet<usize> = if self.hl_linenostart {
            self.hl_lines.iter().filter_map(|&n| (n + 1).checked_sub(self.linenostart)).collect()
        } else {
            self.hl_lines.clone()
        };
        let hl_idx = (!hl_lines.is_empty()).then(|| push_color(&mut colors, hl_col));
        for (_, st) in self.style.iter() {
            for col in [st.color, st.background, st.border].into_iter().flatten() {
                push_color(&mut colors, col);
            }
        }

        let mut font_esc = String::new();
        if !self.fontface.is_empty() {
            font_esc.push(' ');
            for ch in self.fontface.chars() {
                match ch {
                    '\\' => font_esc.push_str("\\\\"),
                    '{' => font_esc.push_str("\\{"),
                    '}' => font_esc.push_str("\\}"),
                    c => font_esc.push(c),
                }
            }
        }
        writeln!(out, "{{\\rtf1\\ansi\\uc0\\deff0{{\\fonttbl{{\\f0\\fmodern\\fprq1\\fcharset0{font_esc};}}}}")?;
        writeln!(out, "{{\\colortbl;")?;
        for c in &colors {
            writeln!(out, "\\red{}\\green{}\\blue{};", c.r, c.g, c.b)?;
        }
        writeln!(out, "}}")?;
        writeln!(out, "\\f0\\sa0")?;
        if self.fontsize > 0 {
            writeln!(out, "\\fs{}", self.fontsize)?;
        }
        writeln!(out, "\\dntblnsbdb")?;

        let mut split_tokens = Vec::new();
        for (ty, val) in tokens {
            if val == "\n" {
                split_tokens.push((ty, "\n".to_owned()));
            } else if val.contains('\n') {
                let mut parts: Vec<&str> = val.split('\n').collect();
                let tail = parts.pop().unwrap_or("");
                for p in parts {
                    split_tokens.push((ty, format!("{p}\n")));
                }
                if !tail.is_empty() {
                    split_tokens.push((ty, tail.to_owned()));
                }
            } else {
                split_tokens.push((ty, val.to_owned()));
            }
        }

        let line_count = split_tokens.iter().filter(|(_, v)| v.ends_with('\n')).count();
        let linenos_width = (line_count + self.linenostart.saturating_sub(1)).to_string().len();
        let padding = " ".repeat(self.lineno_padding);
        let lineno_fs = self.lineno_fontsize.unwrap_or(self.fontsize);

        let mut lineno = 1usize;
        let mut start_new_line = true;
        for (ty, val) in split_tokens {
            if start_new_line
                && hl_lines.contains(&lineno)
                && let Some(idx) = hl_idx
            {
                write!(out, "{{\\highlight{idx} ")?;
            }
            if start_new_line && let Some(idx) = lineno_idx {
                let cur = lineno + self.linenostart - 1;
                let num_str = if cur % self.linenostep == 0 {
                    format!("{cur:>linenos_width$}")
                } else {
                    " ".repeat(linenos_width)
                };
                if lineno_fs != self.fontsize {
                    write!(out, "{{\\fs{lineno_fs} \\cf{idx} {num_str}{padding}}}")?;
                } else {
                    write!(out, "{{\\cf{idx} {num_str}{padding}}}")?;
                }
            }

            let resolved = resolve_token_style(self.style, ty);
            let st = self.style.style_for(resolved);
            let color_idx = |c| colors.iter().position(|&x| x == c).map(|i| i + 1);
            let mut tags = String::new();
            use fmt::Write as _;
            if let Some(idx) = st.background.and_then(color_idx) {
                let _ = write!(tags, "\\cb{idx}");
            }
            if let Some(idx) = st.color.and_then(color_idx) {
                let _ = write!(tags, "\\cf{idx}");
            }
            if st.bold {
                tags.push_str("\\b");
            }
            if st.italic {
                tags.push_str("\\i");
            }
            if st.underline {
                tags.push_str("\\ul");
            }
            if let Some(idx) = st.border.and_then(color_idx) {
                let _ = write!(tags, "\\chbrdr\\chcfpat{idx}");
            }

            if !tags.is_empty() {
                write!(out, "{{{tags} ")?;
            }
            let mut escaped = String::new();
            escape_rtf_text(&val, &mut escaped);
            out.write_str(&escaped)?;
            if !tags.is_empty() {
                out.write_char('}')?;
            }
            start_new_line = false;

            if val.ends_with('\n') {
                if hl_lines.contains(&lineno) {
                    out.write_char('}')?;
                }
                out.write_char('\n')?;
                start_new_line = true;
                lineno += 1;
            }
        }
        out.write_str("}\n")
    }
}
