//! LaTeX `fancyvrb` formatter, ported from `pygments.formatters.latex`.

use std::fmt;

use crate::style::{Color, Style, default_style};
use crate::token::TokenType;

use super::Formatter;

fn escape_tex(text: &str, cp: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str(&format!("\\{cp}Zbs{{}}")),
            '{' => out.push_str(&format!("\\{cp}Zob{{}}")),
            '}' => out.push_str(&format!("\\{cp}Zcb{{}}")),
            '^' => out.push_str(&format!("\\{cp}Zca{{}}")),
            '_' => out.push_str(&format!("\\{cp}Zus{{}}")),
            '&' => out.push_str(&format!("\\{cp}Zam{{}}")),
            '<' => out.push_str(&format!("\\{cp}Zlt{{}}")),
            '>' => out.push_str(&format!("\\{cp}Zgt{{}}")),
            '#' => out.push_str(&format!("\\{cp}Zsh{{}}")),
            '%' => out.push_str(&format!("\\{cp}Zpc{{}}")),
            '$' => out.push_str(&format!("\\{cp}Zdl{{}}")),
            '-' => out.push_str(&format!("\\{cp}Zhy{{}}")),
            '\'' => out.push_str(&format!("\\{cp}Zsq{{}}")),
            '"' => out.push_str(&format!("\\{cp}Zdq{{}}")),
            '~' => out.push_str(&format!("\\{cp}Zti{{}}")),
            _ => out.push(ch),
        }
    }
    out
}

fn tex_ttype_name(ty: TokenType) -> String {
    if ty.is_standard() {
        return ty.css_class().to_owned();
    }
    let mut suffix = String::new();
    let mut cur = ty;
    while !cur.is_standard() {
        let last = cur.name().rsplit('.').next().unwrap_or("");
        suffix = format!("{last}{suffix}");
        cur = cur.parent().unwrap_or(TokenType::TOKEN);
    }
    format!("{}{suffix}", cur.css_class())
}

fn rgb_triplet(c: Option<Color>) -> String {
    match c {
        Some(c) => format!("{:.2},{:.2},{:.2}", f64::from(c.r) / 255.0, f64::from(c.g) / 255.0, f64::from(c.b) / 255.0),
        None => "1,1,1".into(),
    }
}

/// Format tokens as LaTeX `fancyvrb` environments and `\PY{...}{...}` macros.
#[derive(Clone, Debug)]
pub struct LatexFormatter {
    style: &'static Style,
    full: bool,
    title: String,
    nowrap: bool,
    docclass: String,
    preamble: String,
    linenos: bool,
    linenostart: usize,
    linenostep: usize,
    verboptions: String,
    commandprefix: String,
    texcomments: bool,
    mathescape: bool,
    escapeinside: Option<(char, char)>,
    envname: String,
    cmd2def: Vec<(String, String)>,
}

impl Default for LatexFormatter {
    fn default() -> Self {
        Self::new(default_style())
    }
}

impl LatexFormatter {
    /// Construct a [`LatexFormatter`] using `style`.
    #[must_use]
    pub fn new(style: &'static Style) -> Self {
        let mut f = Self {
            style,
            full: false,
            title: String::new(),
            nowrap: false,
            docclass: "article".into(),
            preamble: String::new(),
            linenos: false,
            linenostart: 1,
            linenostep: 1,
            verboptions: String::new(),
            commandprefix: "PY".into(),
            texcomments: false,
            mathescape: false,
            escapeinside: None,
            envname: "Verbatim".into(),
            cmd2def: Vec::new(),
        };
        f.rebuild_stylesheet();
        f
    }

    /// Emit a complete `\documentclass{...}...\end{document}` LaTeX file.
    #[must_use]
    pub const fn with_full(mut self) -> Self {
        self.full = true;
        self
    }

    /// Section heading emitted in `full` mode.
    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Omit the surrounding `\begin{Verbatim}...\end{Verbatim}` environment.
    #[must_use]
    pub const fn with_nowrap(mut self) -> Self {
        self.nowrap = true;
        self
    }

    /// Document class name when `full` is enabled (default `"article"`).
    #[must_use]
    pub fn with_docclass(mut self, docclass: impl Into<String>) -> Self {
        self.docclass = docclass.into();
        self
    }

    /// Extra LaTeX preamble declarations for `full` mode.
    #[must_use]
    pub fn with_preamble(mut self, preamble: impl Into<String>) -> Self {
        self.preamble = preamble.into();
        self
    }

    /// Enable line numbers (`numbers=left`).
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

    /// Line number step interval (default `1`).
    #[must_use]
    pub const fn with_linenostep(mut self, step: usize) -> Self {
        self.linenostep = step;
        self
    }

    /// Additional `fancyvrb` key-value options.
    #[must_use]
    pub fn with_verboptions(mut self, opts: impl Into<String>) -> Self {
        self.verboptions = opts.into();
        self
    }

    /// Command prefix replacing `\PY` (default `"PY"`).
    #[must_use]
    pub fn with_commandprefix(mut self, prefix: impl Into<String>) -> Self {
        self.commandprefix = prefix.into();
        self.rebuild_stylesheet();
        self
    }

    /// Allow raw LaTeX inside comment tokens.
    #[must_use]
    pub const fn with_texcomments(mut self) -> Self {
        self.texcomments = true;
        self
    }

    /// Allow `$...$` LaTeX math mode inside comment tokens.
    #[must_use]
    pub const fn with_mathescape(mut self) -> Self {
        self.mathescape = true;
        self
    }

    /// Two-character delimiter enabling inline LaTeX escapes outside strings.
    #[must_use]
    pub fn with_escapeinside(mut self, delim: &str) -> Self {
        let mut chars = delim.chars();
        self.escapeinside = match (chars.next(), chars.next(), chars.next()) {
            (Some(l), Some(r), None) => Some((l, r)),
            _ => None,
        };
        self
    }

    /// Alternate environment name replacing `"Verbatim"`.
    #[must_use]
    pub fn with_envname(mut self, envname: impl Into<String>) -> Self {
        self.envname = envname.into();
        self
    }

    fn rebuild_stylesheet(&mut self) {
        self.cmd2def.clear();
        let cp = &self.commandprefix;
        for (ty, st) in self.style.iter() {
            let name = tex_ttype_name(ty);
            let mut def = String::new();
            if st.bold {
                def.push_str(r"\let\$$@bf=\textbf");
            }
            if st.italic {
                def.push_str(r"\let\$$@it=\textit");
            }
            if st.underline {
                def.push_str(r"\let\$$@ul=\underline");
            }
            if st.roman {
                def.push_str(r"\let\$$@ff=\textrm");
            }
            if st.sans {
                def.push_str(r"\let\$$@ff=\textsf");
            }
            if st.mono {
                def.push_str(r"\let\$$@ff=\textsf");
            }
            if let Some(c) = st.color {
                def.push_str(&format!(r"\def\$$@tc##1{{\textcolor[rgb]{{{}}}{{##1}}}}", rgb_triplet(Some(c))));
            }
            if let Some(border) = st.border {
                def.push_str(&format!(
                    r"\def\$$@bc##1{{{{\setlength{{\fboxsep}}{{\string -\fboxrule}}\fcolorbox[rgb]{{{}}}{{{}}}{{\strut ##1}}}}}}",
                    rgb_triplet(Some(border)),
                    rgb_triplet(st.background),
                ));
            } else if let Some(bg) = st.background {
                def.push_str(&format!(
                    r"\def\$$@bc##1{{{{\setlength{{\fboxsep}}{{0pt}}\colorbox[rgb]{{{}}}{{\strut ##1}}}}}}",
                    rgb_triplet(Some(bg)),
                ));
            }
            if def.is_empty() {
                continue;
            }
            let def = def.replace("$$", cp);
            self.cmd2def.push((name, def));
        }
    }

    fn format_value(&self, ty: TokenType, val: &str) -> String {
        let cp = &self.commandprefix;
        if ty.is_subtype_of(TokenType::COMMENT) {
            if self.texcomments {
                let mut chars = val.chars();
                if let Some(first) = chars.next() {
                    let prefix_len = val.chars().take_while(|&c| c == first).count();
                    let byte_end = val.char_indices().nth(prefix_len).map_or(val.len(), |(i, _)| i);
                    return format!("{}{}", escape_tex(&val[..byte_end], cp), &val[byte_end..]);
                }
                return String::new();
            }
            if self.mathescape {
                let mut out = String::new();
                for (i, part) in val.split('$').enumerate() {
                    if i > 0 {
                        out.push('$');
                    }
                    if i % 2 == 0 {
                        out.push_str(&escape_tex(part, cp));
                    } else {
                        out.push_str(part);
                    }
                }
                return out;
            }
            if let Some((l, r)) = self.escapeinside {
                let mut text = val;
                let mut out = String::new();
                while !text.is_empty() {
                    if let Some((a, rest1)) = text.split_once(l) {
                        if let Some((b, rest2)) = rest1.split_once(r) {
                            out.push_str(&escape_tex(a, cp));
                            out.push_str(b);
                            text = rest2;
                        } else {
                            out.push_str(&escape_tex(&format!("{a}{l}{rest1}"), cp));
                            break;
                        }
                    } else {
                        out.push_str(&escape_tex(text, cp));
                        break;
                    }
                }
                return out;
            }
        }
        if ty.is_subtype_of(TokenType::ESCAPE) { val.to_owned() } else { escape_tex(val, cp) }
    }

    fn write_body(&self, tokens: &mut dyn Iterator<Item = (TokenType, &str)>, out: &mut dyn fmt::Write) -> fmt::Result {
        let cp = &self.commandprefix;
        if !self.nowrap {
            write!(out, "\\begin{{{}}}[commandchars=\\\\\\{{\\}}", self.envname)?;
            if self.linenos {
                out.write_str(",numbers=left")?;
                if self.linenostart != 0 {
                    write!(out, ",firstnumber={}", self.linenostart)?;
                }
                if self.linenostep != 0 {
                    write!(out, ",stepnumber={}", self.linenostep)?;
                }
            }
            if self.mathescape || self.texcomments || self.escapeinside.is_some() {
                out.write_str(r",codes={\catcode`\$=3\catcode`\^=7\catcode`\_=8\relax}")?;
            }
            if !self.verboptions.is_empty() {
                write!(out, ",{}", self.verboptions)?;
            }
            out.write_str("]\n")?;
        }

        for (ty, val) in tokens {
            let formatted = self.format_value(ty, val);
            let mut classes: Vec<String> =
                ty.ancestors().take_while(|&t| t != TokenType::TOKEN).map(tex_ttype_name).collect();
            classes.reverse();
            let styleval = classes.join("+");
            if styleval.is_empty() {
                out.write_str(&formatted)?;
            } else {
                let mut parts = formatted.split('\n').peekable();
                while let Some(line) = parts.next() {
                    if parts.peek().is_some() {
                        if !line.is_empty() {
                            write!(out, "\\{cp}{{{styleval}}}{{{line}}}")?;
                        }
                        out.write_char('\n')?;
                    } else if !line.is_empty() {
                        write!(out, "\\{cp}{{{styleval}}}{{{line}}}")?;
                    }
                }
            }
        }

        if !self.nowrap {
            writeln!(out, "\\end{{{}}}", self.envname)?;
        }
        Ok(())
    }
}

impl Formatter for LatexFormatter {
    fn name(&self) -> &'static str {
        "LaTeX"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["latex", "tex"]
    }

    fn filenames(&self) -> &'static [&'static str] {
        &["*.tex"]
    }

    fn style_defs(&self, _arg: &str) -> String {
        let cp = &self.commandprefix;
        let styles: String = self
            .cmd2def
            .iter()
            .map(|(name, def)| format!("\\@namedef{{{cp}@tok@{name}}}{{{def}}}"))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "\n\\makeatletter\n\\def\\{cp}@reset{{\\let\\{cp}@it=\\relax \\let\\{cp}@bf=\\relax%\n    \\let\\{cp}@ul=\\relax \\let\\{cp}@tc=\\relax%\n    \\let\\{cp}@bc=\\relax \\let\\{cp}@ff=\\relax}}\n\\def\\{cp}@tok#1{{\\csname {cp}@tok@#1\\endcsname}}\n\\def\\{cp}@toks#1+{{\\ifx\\relax#1\\empty\\else%\n    \\{cp}@tok{{#1}}\\expandafter\\{cp}@toks\\fi}}\n\\def\\{cp}@do#1{{\\{cp}@bc{{\\{cp}@tc{{\\{cp}@ul{{%\n    \\{cp}@it{{\\{cp}@bf{{\\{cp}@ff{{#1}}}}}}}}}}}}\n\\def\\{cp}#1#2{{\\{cp}@reset\\{cp}@toks#1+\\relax+\\{cp}@do{{#2}}}}\n\n{styles}\n\n\\def\\{cp}Zbs{{\\char`\\\\}}\n\\def\\{cp}Zus{{\\char`\\_}}\n\\def\\{cp}Zob{{\\char`\\{{}}\n\\def\\{cp}Zcb{{\\char`\\}}}}\n\\def\\{cp}Zca{{\\char`\\^}}\n\\def\\{cp}Zam{{\\char`\\&}}\n\\def\\{cp}Zlt{{\\char`\\<}}\n\\def\\{cp}Zgt{{\\char`\\>}}\n\\def\\{cp}Zsh{{\\char`\\#}}\n\\def\\{cp}Zpc{{\\char`\\%}}\n\\def\\{cp}Zdl{{\\char`\\$}}\n\\def\\{cp}Zhy{{\\char`\\-}}\n\\def\\{cp}Zsq{{\\char`\\'}}\n\\def\\{cp}Zdq{{\\char`\\\"}}\n\\def\\{cp}Zti{{\\char`\\~}}\n% for compatibility with earlier versions\n\\def\\{cp}Zat{{@}}\n\\def\\{cp}Zlb{{[}}\n\\def\\{cp}Zrb{{]}}\n\\makeatother\n"
        )
    }

    fn format_tokens(
        &self,
        tokens: &mut dyn Iterator<Item = (TokenType, &str)>,
        out: &mut dyn fmt::Write,
    ) -> fmt::Result {
        if self.full {
            let mut code = String::new();
            self.write_body(tokens, &mut code)?;
            let styledefs = self.style_defs("");
            write!(
                out,
                "\n\\documentclass{{{}}}\n\\usepackage{{fancyvrb}}\n\\usepackage{{color}}\n\\usepackage[utf8]{{inputenc}}\n{}\n\n{styledefs}\n\n\\begin{{document}}\n\n\\section*{{{}}}\n\n{code}\\end{{document}}\n",
                self.docclass, self.preamble, self.title
            )
        } else {
            self.write_body(tokens, out)
        }
    }
}
