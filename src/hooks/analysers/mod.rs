//! Hand-ported `analyse_text` heuristics the converter could not express,
//! keyed by the Pygments class that defines them (`{"custom": ...}` in
//! `data/analyse.json`).
//!
//! Each port returns Python's raw value as `f64` (`True` is 1.0, `None`,
//! `False` and a returned `re.Match` are 0.0); [`crate::analyse::score`]
//! clamps it like `pygments.util.make_analysator`. Other lexers' heuristics
//! are called through `score`, which is what calling the wrapped
//! `OtherLexer.analyse_text` does in Python.

use std::sync::LazyLock;

use onig::{Regex, Region, SearchOptions};

use super::{AnalyseFn, Module};
use crate::analyse::{html_doctype_matches, looks_like_xml, score, shebang_matches};
use crate::engine::compile;

mod googlesql_words;

/// A `&'static Regex` compiled once (Python syntax and flag letters).
macro_rules! re {
    ($p:expr) => {
        re!($p, "")
    };
    ($p:expr, $f:expr) => {{
        static R: LazyLock<Regex> = LazyLock::new(|| compile($p, $f).expect("valid regex"));
        &*R
    }};
}

/// Wraps each `fn(&str) -> f64` port into an [`AnalyseFn`].
macro_rules! table {
    ($($key:literal => $f:expr),* $(,)?) => {
        &[$(($key, {
            fn w(t: &str) -> f32 {
                let f: fn(&str) -> f64 = $f;
                f(t) as f32
            }
            w as AnalyseFn
        })),*]
    };
}

pub(crate) static MODULE: Module = Module {
    analyse: table! {
        // AntlrLexer.analyse_text returns a re.Match, which make_analysator turns
        // into 0.0, so every `AntlrLexer.analyse_text(text) and ...` is 0.0.
        "AntlrActionScriptLexer" => zero,
        "AntlrCSharpLexer" => zero,
        "AntlrCppLexer" => zero,
        "AntlrJavaLexer" => zero,
        "AntlrObjectiveCLexer" => zero,
        "AntlrPerlLexer" => zero,
        "AntlrPythonLexer" => zero,
        "AntlrRubyLexer" => zero,
        "BrainfuckLexer" => brainfuck,
        "CMakeLexer" => cmake,
        "CSharpAspxLexer" => csharp_aspx,
        "CarbonLexer" => carbon,
        "ComponentPascalLexer" => component_pascal,
        "CssDjangoLexer" => |t| s("DjangoLexer", t) - 0.05,
        "CssErbLexer" => |t| s("ErbLexer", t) - 0.05,
        "CssGenshiLexer" => |t| s("GenshiLexer", t) - 0.05,
        "CssPhpLexer" => |t| s("PhpLexer", t) - 0.05,
        "CssSmartyLexer" => |t| s("SmartyLexer", t) - 0.05,
        "DesktopLexer" => desktop,
        "DiffLexer" => diff,
        "DjangoLexer" => django,
        "ECLLexer" => ecl,
        "EasytrieveLexer" => easytrieve,
        "EvoqueHtmlLexer" => |t| s("EvoqueLexer", t),
        "EvoqueXmlLexer" => |t| s("EvoqueLexer", t),
        "EzhilLexer" => ezhil,
        "FSharpLexer" => fsharp,
        "GAPLexer" => gap,
        "GDScriptLexer" => gdscript,
        "GasLexer" => gas,
        "GeneratedObjectiveCVariant" => objective_c,
        "GenshiLexer" => |t| genshi_marks(t) + s("XmlLexer", t) - 0.01,
        "GoogleSqlLexer" => google_sql,
        "GroffLexer" => groff,
        "HtmlDjangoLexer" => |t| s("DjangoLexer", t) - 0.01 + html_bonus(t),
        "HtmlGenshiLexer" => |t| genshi_marks(t) + s("HtmlLexer", t) - 0.01,
        "HtmlPhpLexer" => |t| s("PhpLexer", t) - 0.01 + html_bonus(t),
        "HtmlSmartyLexer" => |t| s("SmartyLexer", t) - 0.01 + html_bonus(t),
        "HttpLexer" => http,
        "HybrisLexer" => hybris,
        "IDLLexer" => idl,
        "Inform6Lexer" => inform6,
        "IniLexer" => ini,
        "JagsLexer" => jags,
        "JasminLexer" => jasmin,
        "JavascriptDjangoLexer" => |t| s("DjangoLexer", t) - 0.05,
        "JavascriptErbLexer" => |t| s("ErbLexer", t) - 0.05,
        "JavascriptGenshiLexer" => |t| s("GenshiLexer", t) - 0.05,
        "JavascriptPhpLexer" => |t| s("PhpLexer", t),
        "JavascriptSmartyLexer" => |t| s("SmartyLexer", t) - 0.05,
        "JclLexer" => jcl,
        "JspLexer" => |t| s("JavaLexer", t) - 0.01 + xml_bonus(t) + asp_tags(t),
        "LassoCssLexer" => lasso_css,
        "LassoHtmlLexer" => |t| s("LassoLexer", t) - 0.01 + html_bonus(t),
        "LassoJavascriptLexer" => |t| s("LassoLexer", t) - 0.05,
        "LassoLexer" => lasso,
        "LassoXmlLexer" => |t| s("LassoLexer", t) - 0.01 + xml_bonus(t),
        "LogtalkLexer" => logtalk,
        "MasonLexer" => mason,
        "MatlabLexer" => matlab,
        "MaximaLexer" => maxima,
        "Modula2Lexer" => modula2,
        "MojoLexer" => mojo,
        "MySqlLexer" => mysql,
        "NemerleLexer" => |t| sum_contains(t, &[("@if", 0.1)]),
        "NixLexer" => nix,
        "NotmuchLexer" => |t: &str| bool_score(t.starts_with("\x0cmessage{")),
        "NumPyLexer" => numpy,
        "OpenEdgeLexer" => |t| sum_contains(t, &[("END.", 0.05), ("END PROCEDURE.", 0.05), ("ELSE DO:", 0.05)]),
        "Perl6Lexer" => perl6,
        "PerlLexer" => perl,
        "PhpLexer" => php,
        "PovrayLexer" => |t| sum_contains(t, &[("#version", 0.05), ("#declare", 0.05), ("camera", 0.05), ("light_source", 0.1)]),
        "PythonLexer" => |t| bool_score(python_like(t)),
        "RebolLexer" => rebol,
        "RexxLexer" => rexx,
        "RhtmlLexer" => |t| s("ErbLexer", t) - 0.01 + html_bonus(t),
        "RslLexer" => |t| bool_score(search(re!(r"scheme\s*.*?=\s*class\s*type", "i"), t)),
        "RstLexer" => rst,
        "ScdocLexer" => scdoc,
        "ScilabLexer" => scilab,
        "SingularityLexer" => singularity,
        "SmaliLexer" => smali,
        "SmartyLexer" => smarty,
        "SourcesListLexer" => sources_list,
        "SqlJinjaLexer" => sql_jinja,
        "SspLexer" => ssp,
        "SwigLexer" => swig,
        "SystemdLexer" => systemd,
        "Tads3Lexer" => tads3,
        "TalLexer" => |t| bool_score(head(t, 500).contains("|0100")),
        "TeaTemplateLexer" => |t| s("TeaLangLexer", t) - 0.01 + xml_bonus(t) + asp_tags(t),
        "TexLexer" => tex,
        "TransactSqlLexer" => transact_sql,
        "TurtleLexer" => turtle,
        "UcodeLexer" => ucode,
        "UrbiscriptLexer" => |t| sum_contains(t, &[("freezeif", 0.05), ("waituntil", 0.05)]),
        "VCLLexer" => vcl,
        "VbNetAspxLexer" => vbnet_aspx,
        "VelocityLexer" => velocity,
        "VelocityXmlLexer" => |t| s("VelocityLexer", t) - 0.01 + xml_bonus(t),
        "VerilogLexer" => |t| sum_contains(t, &[("reg", 0.1), ("wire", 0.1), ("assign", 0.1)]),
        "WoWTocLexer" => wowtoc,
        "XmlDjangoLexer" => |t| s("DjangoLexer", t) - 0.01 + xml_bonus(t),
        "XmlErbLexer" => |t| s("ErbLexer", t) - 0.01 + xml_bonus(t),
        "XmlPhpLexer" => |t| s("PhpLexer", t) - 0.01 + xml_bonus(t),
        "XmlSmartyLexer" => |t| s("SmartyLexer", t) - 0.01 + xml_bonus(t),
    },
    ..Module::EMPTY
};

// -- Python helpers ---------------------------------------------------------------

/// `str.isspace` for one character (Rust's set lacks `\x1c`..`\x1f`).
fn py_space(c: char) -> bool {
    c.is_whitespace() || ('\x1c'..='\x1f').contains(&c)
}

/// `str.split()` with no argument.
fn py_words(t: &str) -> impl Iterator<Item = &str> {
    t.split(py_space).filter(|w| !w.is_empty())
}

/// `str.strip()` with no argument.
fn py_strip(t: &str) -> &str {
    t.trim_matches(py_space)
}

/// `str.splitlines()`.
fn py_splitlines(t: &str) -> impl Iterator<Item = &str> {
    let mut rest = t;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let brk = rest.char_indices().find(|&(_, c)| {
            matches!(c, '\n' | '\r' | '\x0b' | '\x0c' | '\x1c' | '\x1d' | '\x1e' | '\u{85}' | '\u{2028}' | '\u{2029}')
        });
        let Some((i, c)) = brk else {
            return Some(std::mem::take(&mut rest));
        };
        let line = &rest[..i];
        let mut j = i + c.len_utf8();
        if c == '\r' && rest[j..].starts_with('\n') {
            j += 1;
        }
        rest = &rest[j..];
        Some(line)
    })
}

/// `text[:n]` (character based).
fn head(t: &str, n: usize) -> &str {
    t.char_indices().nth(n).map_or(t, |(i, _)| &t[..i])
}

/// `re.search(...) is not None`.
fn search(r: &Regex, t: &str) -> bool {
    r.find(t).is_some()
}

/// `re.match(...) is not None` (anchored at the start, not the whole text).
fn match_start(r: &Regex, t: &str) -> bool {
    r.match_with_options(t, 0, SearchOptions::SEARCH_OPTION_NONE, None).is_some()
}

fn bool_score(b: bool) -> f64 {
    if b { 1.0 } else { 0.0 }
}

/// Another lexer's (clamped) `analyse_text`.
fn s(class: &str, t: &str) -> f64 {
    f64::from(score(class, t))
}

/// `result = 0; if 'x' in text: result += w; ...`, summed in Python's order.
fn sum_contains(t: &str, items: &[(&str, f64)]) -> f64 {
    items.iter().filter(|(w, _)| t.contains(w)).fold(0.0, |a, (_, v)| a + v)
}

fn html_bonus(t: &str) -> f64 {
    if html_doctype_matches(t) { 0.5 } else { 0.0 }
}

fn xml_bonus(t: &str) -> f64 {
    if looks_like_xml(t) { 0.4 } else { 0.0 }
}

fn asp_tags(t: &str) -> f64 {
    if t.contains("<%") && t.contains("%>") { 0.1 } else { 0.0 }
}

fn zero(_: &str) -> f64 {
    0.0
}

// -- ports ------------------------------------------------------------------------

fn brainfuck(t: &str) -> f64 {
    // text[:max(256, len(text))] is all of text.
    let range = t.chars().count().max(256) as f64;
    let plus_minus = t.bytes().filter(|b| matches!(b, b'+' | b'-')).count() as f64;
    let greater_less = t.bytes().filter(|b| matches!(b, b'<' | b'>')).count() as f64;
    if plus_minus > 0.25 * range || greater_less > 0.25 * range {
        return 1.0;
    }
    if t.contains("[-]") { 0.5 } else { 0.0 }
}

fn cmake(t: &str) -> f64 {
    let r = re!(
        r"^[ \t]*CMAKE_MINIMUM_REQUIRED[ \t]*\([ \t]*VERSION[ \t]*\d+(\.\d+)*[ \t]*([ \t]FATAL_ERROR)?[ \t]*\)[ \t]*(#[^\n]*)?$",
        "mi"
    );
    if search(r, t) { 0.8 } else { 0.0 }
}

fn csharp_aspx(t: &str) -> f64 {
    if search(re!(r#"Page\s*Language="C#""#, "i"), t) {
        0.2
    } else if search(re!(r#"script[^>]+language=["']C#"#, "i"), t) {
        0.15
    } else {
        0.0
    }
}

fn vbnet_aspx(t: &str) -> f64 {
    if search(re!(r#"Page\s*Language="Vb""#, "i"), t) {
        0.2
    } else if search(re!(r#"script[^>]+language=["']vb"#, "i"), t) {
        0.15
    } else {
        0.0
    }
}

fn carbon(t: &str) -> f64 {
    let words = ["forall", "type", "Self", "observe", "package", "library", "choice", "addr", "constraint", "impl"];
    words.iter().filter(|w| t.contains(*w)).fold(0.0, |a, _| a + 0.1)
}

fn component_pascal(t: &str) -> f64 {
    sum_contains(t, &[("BEGIN", 0.01), ("END", 0.01), ("PROCEDURE", 0.01), ("END", 0.01)])
}

fn ecl(t: &str) -> f64 {
    sum_contains(t, &[("->", 0.01), ("BEGIN", 0.01), ("END", 0.01)])
}

fn fsharp(t: &str) -> f64 {
    sum_contains(t, &[("|>", 0.05), ("<|", 0.05)])
}

fn idl(t: &str) -> f64 {
    sum_contains(t, &[("endelse", 0.2), ("endswitch", 0.01)])
}

fn tads3(t: &str) -> f64 {
    let mut r = 0.0;
    if t.contains("__TADS") || t.contains("GameMainDef") {
        r += 0.2;
    }
    if t.contains("versionInfo") && t.contains("IFID") {
        r += 0.1;
    }
    r
}

fn ucode(t: &str) -> f64 {
    let mut r = sum_contains(t, &[("endsuspend", 0.1), ("endrepeat", 0.1), (":=", 0.01)]);
    if t.contains("procedure") && t.contains("end") {
        r += 0.01;
    }
    if t.contains("\\self") && t.contains("/self") {
        r += 0.5;
    }
    r
}

fn desktop(t: &str) -> f64 {
    if t.starts_with("[Desktop Entry]") {
        1.0
    } else if search(re!(r"^\[Desktop Entry\][ \t]*$", "m"), head(t, 500)) {
        0.9
    } else {
        0.0
    }
}

fn systemd(t: &str) -> f64 {
    if t.starts_with("[Unit]") {
        1.0
    } else if search(re!(r"^\[Unit\][ \t]*$", "m"), head(t, 500)) {
        0.9
    } else {
        0.0
    }
}

fn diff(t: &str) -> f64 {
    if t.starts_with("Index: ") || t.starts_with("diff ") {
        1.0
    } else if t.starts_with("--- ") {
        0.9
    } else {
        0.0
    }
}

fn django(t: &str) -> f64 {
    let mut rv = 0.0;
    if search(re!(r"\{%\s*(block|extends)"), t) {
        rv += 0.4;
    }
    if search(re!(r"\{%\s*if\s*.*?%\}"), t) {
        rv += 0.1;
    }
    if search(re!(r"\{\{.*?\}\}"), t) {
        rv += 0.1;
    }
    rv
}

fn easytrieve(t: &str) -> f64 {
    let is_empty = |l: &str| py_strip(l).is_empty();
    let mut lines = t.split('\n');
    let mut has_header_comment = false;
    let mut first = None;
    // Skip leading empty lines and header comments.
    for line in lines.by_ref() {
        if is_empty(line) {
            continue;
        }
        if match_start(re!(r"^\s*\*"), line) {
            has_header_comment = true;
            continue;
        }
        first = Some(line);
        break;
    }
    // Python indexes lines[0] of an emptied list: IndexError, so 0.0.
    let Some(first) = first else { return 0.0 };
    if match_start(re!(r"^\s*MACRO"), first) {
        return if has_header_comment { 0.4 + 0.4 } else { 0.4 };
    }
    let (mut has_end_proc, mut has_file, mut has_job, mut has_proc, mut has_parm, mut has_report) =
        (false, false, false, false, false, false);
    for line in std::iter::once(first).chain(lines) {
        let mut words = py_words(line);
        let (Some(w), Some(_)) = (words.next(), words.next()) else { continue };
        if has_report {
            continue;
        }
        if !has_job {
            if !has_file {
                if !has_parm && w == "PARM" {
                    has_parm = true;
                }
                if w == "FILE" {
                    has_file = true;
                }
            }
            if w == "JOB" {
                has_job = true;
            }
        } else if w == "PROC" {
            has_proc = true;
        } else if w == "END-PROC" {
            has_end_proc = true;
        } else if w == "REPORT" {
            has_report = true;
        }
    }
    let mut result = 0.0;
    if has_job && has_proc == has_end_proc {
        if has_header_comment {
            result += 0.1;
        }
        if has_parm {
            result += if has_proc { 0.8 } else { 0.5 };
        } else {
            result += 0.11;
            if has_file {
                result += 0.01;
            }
            if has_report {
                result += 0.01;
            }
        }
    }
    result
}

fn ezhil(t: &str) -> f64 {
    if t.chars().filter(|c| ('\u{0b80}'..='\u{0bff}').contains(c)).count() > 10 { 0.25 } else { 0.0 }
}

fn gap(t: &str) -> f64 {
    let mut score = 0.0;
    if search(
        re!(
            r"(InstallTrueMethod|Declare(Attribute|Category|Filter|Operation|GlobalFunction|Synonym|SynonymAttr|Property))"
        ),
        t,
    ) {
        score += 0.7;
    }
    if search(
        re!(
            r"(DeclareRepresentation|Install(GlobalFunction|Method|ImmediateMethod|OtherMethod)|New(Family|Type)|Objectify)"
        ),
        t,
    ) {
        score += 0.7;
    }
    f64::min(score, 1.0)
}

fn gdscript(t: &str) -> f64 {
    let mut score = 0.0;
    if search(re!(r"func (_ready|_init|_input|_process|_unhandled_input)"), t) {
        score += 0.8;
    }
    if search(re!(r"(extends |class_name |onready |preload|load|setget|func [^_])"), t) {
        score += 0.4;
    }
    if search(re!(r"(var|const|enum|export|signal|tool)"), t) {
        score += 0.2;
    }
    f64::min(score, 1.0)
}

fn gas(t: &str) -> f64 {
    if search(re!(r"^\.(text|data|section)", "m"), t) {
        1.0
    } else if search(re!(r"^\.\w+", "m"), t) {
        0.1
    } else {
        0.0
    }
}

fn objective_c(t: &str) -> f64 {
    if search(re!(r"@(?:end|implementation|protocol)"), t) {
        1.0
    } else if t.contains("@\"") {
        0.8
    } else if search(re!(r"@[0-9]+"), t) {
        0.7
    } else if search(re!(r"\[\s*[a-zA-Z_]\w*\s+(?:[a-zA-Z_]\w*\s*\]|(?:[a-zA-Z_]\w*)?:)"), t) {
        0.8
    } else {
        0.0
    }
}

/// The `${...}` / `py:attr=` part shared by `GenshiLexer` and `HtmlGenshiLexer`.
fn genshi_marks(t: &str) -> f64 {
    let mut rv = 0.0;
    if search(re!(r"\$\{.*?\}"), t) {
        rv += 0.2;
    }
    if search(re!(r#"py:(.*?)=["']"#), t) {
        rv += 0.2;
    }
    rv
}

fn google_sql(t: &str) -> f64 {
    let n = py_words(t).filter(|w| googlesql_words::WORDS.binary_search(w).is_ok()).count();
    0.001 * n as f64
}

fn groff(t: &str) -> f64 {
    let mut cs = t.chars();
    if cs.next() != Some('.') {
        return 0.0;
    }
    if t.starts_with(".\\\"") || t.starts_with(".TH ") {
        return 1.0;
    }
    // text[1:3].isalnum() and text[3].isspace(); text[3] raising IndexError gives 0.0.
    let (c1, c2) = (cs.next(), cs.next());
    if c1.is_some_and(char::is_alphanumeric) && c2.is_none_or(char::is_alphanumeric) && cs.next().is_some_and(py_space)
    {
        0.9
    } else {
        0.0
    }
}

fn http(t: &str) -> f64 {
    let request = re!(r"^([a-zA-Z][-_a-zA-Z]+)( +)([^ ]+)( +)(HTTP)(/)(1\.[01]|2(?:\.0)?|3)(\r?\n|\Z)");
    let response = re!(r"^(HTTP)(/)(1\.[01]|2(?:\.0)?|3)( +)(\d{3})(?:( +)([^\r\n]*))?(\r?\n|\Z)");
    bool_score(search(request, t) || search(response, t))
}

fn hybris(t: &str) -> f64 {
    if search(re!(r"\b(?:public|private)\s+method\b"), t) { 0.01 } else { 0.0 }
}

fn inform6(t: &str) -> f64 {
    // The Python pattern is not a raw string: its `\b`s are backspace characters.
    if search(re!("\u{8}origsource\u{8}", "i"), t) { 0.05 } else { 0.0 }
}

fn ini(t: &str) -> f64 {
    let Some(nl) = t.find('\n') else { return 0.0 };
    // npos < 3 compares a character index.
    if t[..nl].chars().count() < 3 {
        return 0.0;
    }
    if t.starts_with('[') && t[..nl].ends_with(']') { 0.8 } else { 0.0 }
}

fn jags(t: &str) -> f64 {
    if !search(re!(r"^\s*model\s*\{", "m"), t) {
        0.0
    } else if search(re!(r"^\s*data\s*\{", "m"), t) || search(re!(r"^\s*var", "m"), t) {
        0.9
    } else {
        0.3
    }
}

fn jasmin(t: &str) -> f64 {
    let mut score = 0.0;
    if search(re!(r"^\s*\.class\s", "m"), t) {
        score += 0.5;
        if search(re!(r"^\s*[a-z]+_[a-z]+\b", "m"), t) {
            score += 0.3;
        }
    }
    if search(
        re!(r"^\s*\.(attribute|bytecode|debug|deprecated|enclosing|inner|interface|limit|set|signature|stack)\b", "m"),
        t,
    ) {
        score += 0.6;
    }
    f64::min(score, 1.0)
}

fn jcl(t: &str) -> f64 {
    let first = t.split('\n').next().unwrap_or("");
    bool_score(match_start(re!(r"^//[a-z#$@][a-z0-9#$@]{0,7}\s+job(\s+.*)?$", "i"), first))
}

fn lasso(t: &str) -> f64 {
    let mut rv = 0.0;
    if t.contains("bin/lasso9") {
        rv += 0.8;
    }
    if search(re!(r"<\?lasso", "i"), t) {
        rv += 0.4;
    }
    if search(re!(r"local\(", "i"), t) {
        rv += 0.4;
    }
    rv
}

fn lasso_css(t: &str) -> f64 {
    let mut rv = s("LassoLexer", t) - 0.05;
    if search(re!(r"\w+:[^;]+;"), t) {
        rv += 0.1;
    }
    if t.contains("padding:") {
        rv += 0.1;
    }
    rv
}

fn logtalk(t: &str) -> f64 {
    if t.contains(":- object(") || t.contains(":- protocol(") || t.contains(":- category(") {
        1.0
    } else if search(re!(r"^:-\s[a-z]", "m"), t) {
        0.9
    } else {
        0.0
    }
}

fn mason(t: &str) -> f64 {
    if search(re!(r"</%(class|doc|init)>"), t) {
        1.0
    } else if search(re!(r"<&.+&>", "s"), t) {
        0.11
    } else {
        0.0
    }
}

fn matlab(t: &str) -> f64 {
    let comment = re!(r"^\s*%");
    // The Python filter tests `text`, not the line: all lines or none pass.
    let first = if match_start(comment, t) { "" } else { py_splitlines(t).next().unwrap_or("") };
    let first = py_strip(first);
    if first.starts_with("function") && !first.contains('{') {
        1.0
    } else if search(re!(r"^\s*%", "m"), t) || search(re!(r"^!\w+", "m"), t) {
        0.2
    } else {
        0.0
    }
}

fn maxima(t: &str) -> f64 {
    let mut strength = 0.0;
    if search(re!(r"\$\s*$", "m"), t) {
        strength += 0.05;
    }
    if t.contains(":=") {
        strength += 0.02;
    }
    strength
}

fn modula2(t: &str) -> f64 {
    if !(t.contains("(*") && t.contains("*)") && t.contains(":=")) {
        return 0.0;
    }
    if search(re!(r"\bFUNCTION\b"), t) {
        0.0
    } else if search(re!(r"\bPROCEDURE\b"), t) {
        0.6
    } else {
        0.0
    }
}

fn mojo(t: &str) -> f64 {
    if shebang_matches(t, re!(r"^mojo?(\.(exe|cmd|bat|bin))?$", "i")) {
        1.0
    } else if head(t, 1000).contains("import ") {
        0.9
    } else {
        0.0
    }
}

/// `(len(name_between_backtick_re.findall(text)), len(name_between_bracket_re.findall(text)))`.
fn dialect_names(t: &str) -> (usize, usize) {
    (re!(r"`[a-zA-Z_]\w*`").find_iter(t).count(), re!(r"\[[a-zA-Z_]\w*\]").find_iter(t).count())
}

fn mysql(t: &str) -> f64 {
    let (backtick, bracket) = dialect_names(t);
    if backtick + bracket >= 1 && backtick >= 2 * bracket {
        0.5
    } else if backtick > bracket {
        0.2
    } else if backtick > 0 {
        0.1
    } else {
        0.0
    }
}

fn transact_sql(t: &str) -> f64 {
    if search(re!(r"\bdeclare\s+@", "i"), t) {
        return 1.0;
    }
    let (backtick, bracket) = dialect_names(t);
    let mut rating = if backtick + bracket >= 1 && bracket >= 2 * backtick {
        0.5
    } else if bracket > backtick {
        0.2
    } else if bracket > 0 {
        0.1
    } else {
        0.0
    };
    if search(re!(r"@[a-zA-Z_]\w*\b"), t) {
        rating += 0.1;
    }
    if search(re!(r"\bgo\b", "i"), t) {
        rating += 0.1;
    }
    rating
}

fn nix(t: &str) -> f64 {
    let mut rv = 0.0;
    if search(re!(r"import.+?<[^>]+>"), t) {
        rv += 0.4;
    }
    if search(re!(r"mkDerivation\s+(\(|\{|rec)"), t) {
        rv += 0.4;
    }
    if search(re!(r"=\s+mkIf\s+"), t) {
        rv += 0.4;
    }
    if search(re!(r"\{[a-zA-Z,\s]+\}:"), t) {
        rv += 0.1;
    }
    rv
}

/// `PythonLexer.analyse_text` before `make_analysator`.
fn python_like(t: &str) -> bool {
    shebang_matches(t, re!(r"^pythonw?(3(\.\d)?)?(\.(exe|cmd|bat|bin))?$", "i")) || head(t, 1000).contains("import ")
}

fn numpy(t: &str) -> f64 {
    let l = head(t, 1000);
    bool_score(python_like(t) && (l.contains("import numpy") || l.contains("from numpy import")))
}

fn perl6(t: &str) -> f64 {
    let (pod_end, pod_start) = (re!(r"^=(?:end|cut)"), re!(r"^=\w+"));
    let mut in_pod = false;
    let lines: Vec<&str> = py_splitlines(t)
        .filter(|l| {
            if match_start(pod_end, l) {
                in_pod = false;
                false
            } else if match_start(pod_start, l) {
                in_pod = true;
                false
            } else {
                !in_pod
            }
        })
        .collect();
    let text = lines.join("\n");
    // Alternation is not grouped in Python either: `^perl6|rakudo|niecza|pugs(...)?$`.
    if shebang_matches(&text, re!(r"^perl6|rakudo|niecza|pugs(\.(exe|cmd|bat|bin))?$", "i")) {
        return 1.0;
    }
    let mut saw_perl_decl = false;
    let mut rating = 0.0;
    if search(re!(r"(?:my|our|has)\s+(?:['\w:-]+\s+)?[$@%&(]"), &text) {
        rating = 0.8;
        saw_perl_decl = true;
    }
    let class_decl = re!(r"^\s*(?:(my|our)\s+)?(?:module|class|role|enum|grammar)");
    for line in &lines {
        let line = line.split('#').next().unwrap_or("");
        if line.chars().all(py_space) {
            continue;
        }
        if match_start(re!(r"^\s*(?:use\s+)?v6(?:\.\d(?:\.\d)?)?;"), line) {
            return 1.0;
        }
        let mut region = Region::new();
        if class_decl.match_with_options(line, 0, SearchOptions::SEARCH_OPTION_NONE, Some(&mut region)).is_some() {
            if saw_perl_decl || region.pos(1).is_some() {
                return 1.0;
            }
            rating = 0.05;
            continue;
        }
        break;
    }
    if text.contains(":=") {
        rating /= 2.0;
    }
    rating
}

fn perl(t: &str) -> f64 {
    if shebang_matches(t, re!(r"^perl(\.(exe|cmd|bat|bin))?$", "i")) {
        return 1.0;
    }
    let mut result = if search(re!(r"(?:my|our)\s+[$@%(]"), t) { 0.9 } else { 0.0 };
    if t.contains(":=") {
        result /= 2.0;
    }
    result
}

fn php(t: &str) -> f64 {
    if shebang_matches(t, re!(r"^php(\.(exe|cmd|bat|bin))?$", "i")) {
        1.0
    } else if search(re!(r"<\?(?!xml)"), t) {
        0.3
    } else {
        0.0
    }
}

fn rebol(t: &str) -> f64 {
    if match_start(re!(r"^\s*REBOL\s*\[", "i"), t) {
        1.0
    } else if search(re!(r"\s*REBOL\s*\[", "i"), t) {
        0.5
    } else {
        0.0
    }
}

fn rexx(t: &str) -> f64 {
    static PATTERNS: LazyLock<[(Regex, f64); 7]> = LazyLock::new(|| {
        let c = |p: &str| compile(p, "m").expect("valid regex");
        [
            (c(r"^\s*address\s+command\b"), 0.2),
            (c(r"^\s*address\s+"), 0.05),
            (c(r"^\s*do\s+while\b"), 0.1),
            (c(r"\belse\s+do\s*$"), 0.1),
            (c(r"^\s*if\b.+\bthen\s+do\s*$"), 0.1),
            (c(r"^\s*([a-z_]\w*)(\s*)(:)(\s*)(procedure)\b"), 0.5),
            (c(r"^\s*parse\s+(upper\s+)?(arg|value)\b"), 0.2),
        ]
    });
    if search(re!(r"/\*\**\s*rexx", "i"), t) {
        return 1.0;
    }
    if !t.starts_with("/*") {
        return 0.0;
    }
    let lower = t.to_lowercase();
    let sum = PATTERNS.iter().filter(|(r, _)| search(r, &lower)).fold(0.0, |a, (_, w)| a + w);
    f64::min(sum + 0.01, 1.0)
}

fn rst(t: &str) -> f64 {
    if t.starts_with("..") && !t[2..].starts_with('.') {
        return 0.3;
    }
    // Two lines of equal (character) length, the second one `---`/`===`-like.
    let Some(p1) = t.find('\n') else { return 0.0 };
    let rest = &t[p1 + 1..];
    let Some(p2) = rest.find('\n') else { return 0.0 };
    let second = &rest[..p2];
    let (Some(first), Some(last)) = (second.chars().next(), second.chars().next_back()) else { return 0.0 };
    if matches!(first, '-' | '=') && first == last && second.chars().count() == t[..p1].chars().count() {
        0.5
    } else {
        0.0
    }
}

fn scdoc(t: &str) -> f64 {
    let mut result = sum_contains(t, &[("*", 0.01), ("_", 0.01)]);
    let first_line = t.split('\n').next().unwrap_or("");
    if search(re!(r#"^.*\([1-7]\)( "[^"]+"){0,2}$"#), first_line) {
        result += 0.5;
    }
    result
}

fn scilab(t: &str) -> f64 {
    let mut score = 0.0;
    if search(re!(r"^\s*//"), t) {
        score += 0.1;
    }
    if search(re!(r"^\s*/\*"), t) {
        score += 0.1;
    }
    f64::min(score, 1.0)
}

fn singularity(t: &str) -> f64 {
    let mut result = 0.0;
    if search(re!(r"\b(?:osversion|includecmd|mirrorurl)\b", "i"), t) {
        result += 0.5;
    }
    // SingularityLexer._section without its leading `^`.
    if search(re!(r"(%(?:pre|post|setup|environment|help|labels|test|runscript|files|startscript))(\s*)"), t) {
        result += 0.49;
    }
    result
}

fn smali(t: &str) -> f64 {
    let mut score = 0.0;
    if search(re!(r"^\s*\.class\s", "m"), t) {
        score += 0.5;
        if search(
            re!(
                r"\b((check-cast|instance-of|throw-verification-error)\b|(-to|add|[ais]get|[ais]put|and|cmpl|const|div|if|invoke|move|mul|neg|not|or|rem|return|rsub|shl|shr|sub|ushr)[-/])|\{|\}",
                "m"
            ),
            t,
        ) {
            score += 0.3;
        }
    }
    if search(
        re!(
            r"(\.(catchall|epilogue|restart local|prologue)|\b(array-data|class-change-error|declared-synchronized|(field|inline|vtable)@0x[0-9a-fA-F]|generic-error|illegal-class-access|illegal-field-access|illegal-method-access|instantiation-error|no-error|no-such-class|no-such-field|no-such-method|packed-switch|sparse-switch))\b",
            "m"
        ),
        t,
    ) {
        score += 0.6;
    }
    score
}

fn smarty(t: &str) -> f64 {
    let mut rv = 0.0;
    if search(re!(r"\{if\s+.*?\}.*?\{/if\}"), t) {
        rv += 0.15;
    }
    if search(re!(r"\{include\s+file=.*?\}"), t) {
        rv += 0.15;
    }
    if search(re!(r"\{foreach\s+.*?\}.*?\{/foreach\}"), t) {
        rv += 0.15;
    }
    if search(re!(r"\{\$.*?\}"), t) {
        rv += 0.01;
    }
    rv
}

fn sources_list(t: &str) -> f64 {
    bool_score(py_splitlines(t).map(py_strip).any(|l| l.starts_with("deb ") || l.starts_with("deb-src ")))
}

fn sql_jinja(t: &str) -> f64 {
    let mut rv = 0.0;
    if search(re!(r"\{\{\s*ref\(.*\)\s*\}\}"), t) {
        rv += 0.4;
    }
    if search(re!(r"\{\{\s*source\(.*\)\s*\}\}"), t) {
        rv += 0.25;
    }
    if search(re!(r"\{%-?\s*macro \w+\(.*\)\s*-?%\}"), t) {
        rv += 0.15;
    }
    rv
}

fn ssp(t: &str) -> f64 {
    let mut rv = 0.0;
    if search(re!(r"val \w+\s*:"), t) {
        rv += 0.6;
    }
    if looks_like_xml(t) {
        rv += 0.2;
    }
    rv + asp_tags(t)
}

fn swig(t: &str) -> f64 {
    let mut rv = 0.0;
    for caps in re!(r"^\s*(%[a-z_][a-z0-9_]*)", "m").captures_iter(t) {
        let Some(m) = caps.at(1) else { continue };
        if crate::registry::by_class("SwigLexer").is_some_and(|l| l.word_set("swig_directives").contains(m)) {
            return 0.98;
        }
        rv = 0.91;
    }
    rv
}

fn tex(t: &str) -> f64 {
    bool_score(["\\documentclass", "\\input", "\\documentstyle", "\\relax"].iter().any(|s| t.starts_with(s)))
}

fn turtle(t: &str) -> f64 {
    // re.search(rf'^\s*{t}', text) without MULTILINE: only at the very start.
    let s = t.trim_start_matches(py_space);
    if ["@base ", "BASE ", "@prefix ", "PREFIX "].iter().any(|p| s.starts_with(p)) { 0.8 } else { 0.0 }
}

fn vcl(t: &str) -> f64 {
    if t.starts_with("vcl 4.0;") {
        1.0
    } else if head(t, 1000).contains("\nvcl 4.0;") {
        0.9
    } else {
        0.0
    }
}

fn velocity(t: &str) -> f64 {
    let mut rv = 0.0;
    if search(re!(r"#\{?macro\}?\(.*?\).*?#\{?end\}?", "s"), t) {
        rv += 0.25;
    }
    if search(re!(r"#\{?if\}?\(.+?\).*?#\{?end\}?", "s"), t) {
        rv += 0.15;
    }
    if search(re!(r"#\{?foreach\}?\(.+?\).*?#\{?end\}?", "s"), t) {
        rv += 0.15;
    }
    if search(re!(r"\$!?\{?[a-zA-Z_]\w*(\([^)]*\))?(\.\w+(\([^)]*\))?)*\}?"), t) {
        rv += 0.01;
    }
    rv
}

fn wowtoc(t: &str) -> f64 {
    let mut result = 0.0;
    let interface = re!(r"^(##)( *)(Interface)( *)(:)( *)(.*?)( *)$", "i");
    if interface.captures(t).and_then(|c| c.at(7)).is_some_and(|v| match_start(re!(r"(\d+)(\d{2})(\d{2})"), v)) {
        result += 0.8;
    }
    // casefold() only matters for ASCII letters here.
    let folded = t.to_lowercase();
    if folded.contains(".lua") {
        result += 0.1;
    }
    if folded.contains(".xml") {
        result += 0.05;
    }
    result
}

#[cfg(test)]
mod tests;
