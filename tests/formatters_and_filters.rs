//! Integration tests for token filters, all 14 pure-text formatters, `LexOptions`,
//! `highlight()`, and the `pygmentize` binary.

use std::io::Write as _;
use std::process::{Command, Stdio};

use pygments_rs::filter::{
    CodeTagFilter, ErrorToken, GobbleFilter, KeywordCase, KeywordCaseFilter, NameHighlightFilter,
    RaiseOnErrorTokenFilter, SymbolFilter, SymbolLang, TokenMergeFilter, VisibleWhitespaceFilter,
};
use pygments_rs::formatters::{
    BBCodeFormatter, GroffFormatter, HtmlFormatter, HtmlLineNumbers, IrcFormatter, LatexFormatter, NullFormatter,
    PangoMarkupFormatter, RawTokenFormatter, RtfFormatter, SvgFormatter, Terminal256Formatter, TerminalBackground,
    TerminalFormatter, TerminalTrueColorFormatter, TestcaseFormatter,
};
use pygments_rs::{
    LexOptions, TokenType, apply_filters, filter_by_name, filter_names, format_string, formatter_by_name,
    formatter_for_filename, formatter_names, highlight, lexer_by_name, style_by_name,
};

#[test]
fn lex_options_and_highlight() {
    let rust = lexer_by_name("rust").unwrap();
    let custom = LexOptions { stripnl: false, stripall: true, ensurenl: false, tabsize: 4 };
    assert_eq!(rust.preprocess_with("\u{feff}  \tlet x = 1;\r\n  ", &custom), "let x = 1;");

    let html = HtmlFormatter::new(style_by_name("monokai").unwrap());
    let rendered = highlight("fn a() {}", rust, &html);
    assert!(rendered.starts_with("<div class=\"highlight\"><pre><span></span>"));
    assert!(rendered.contains("<span class=\"k\">fn</span>"));
    assert!(rendered.ends_with("</pre></div>\n"));
}

#[test]
fn all_eight_filters_and_registry() {
    assert_eq!(filter_names().len(), 8);
    let rust = lexer_by_name("rust").unwrap();
    let src = rust.preprocess("  fn main() { /* TODO check \\<alpha> \\beta */ }\n");
    let toks: Vec<_> = rust.tokenize(&src).collect();

    let codetag = CodeTagFilter::new();
    let filtered = apply_filters(Some(rust), toks.iter().copied(), &[&codetag]).unwrap();
    assert!(filtered.iter().any(|(t, s)| *t == TokenType::COMMENT_SPECIAL && s == "TODO"));

    let sym_iso = SymbolFilter::new(SymbolLang::Isabelle);
    let sym_tex = SymbolFilter::new(SymbolLang::Latex);
    assert_eq!(apply_filters(None, [(TokenType::TEXT, "\\<alpha>")], &[&sym_iso]).unwrap()[0].1, "α");
    assert_eq!(apply_filters(None, [(TokenType::TEXT, "\\beta")], &[&sym_tex]).unwrap()[0].1, "β");

    let kw = KeywordCaseFilter::new(KeywordCase::Upper);
    let upper = apply_filters(Some(rust), toks.iter().copied(), &[&kw]).unwrap();
    assert!(upper.iter().any(|(t, s)| *t == TokenType::KEYWORD && s == "FN"));

    let hl = NameHighlightFilter::new(["main"]).with_token_type(TokenType::NAME_BUILTIN);
    let named = apply_filters(Some(rust), [(TokenType::NAME, "main"), (TokenType::NAME, "x")], &[&hl]).unwrap();
    assert_eq!(named[0].0, TokenType::NAME_BUILTIN);
    assert_eq!(named[1].0, TokenType::NAME);

    let err = apply_filters(None, [(TokenType::ERROR, "bad")], &[&RaiseOnErrorTokenFilter::new()]);
    assert_eq!(err, Err(ErrorToken("bad".into())));

    let ws = VisibleWhitespaceFilter::new()
        .with_default_spaces()
        .with_default_tabs()
        .with_tabsize(4)
        .with_default_newlines();
    let wtoks = apply_filters(None, [(TokenType::TEXT, "a \tb\n")], &[&ws]).unwrap();
    assert_eq!(
        wtoks.iter().map(|(t, s)| (*t, s.as_ref())).collect::<Vec<_>>(),
        vec![
            (TokenType::TEXT, "a"),
            (TokenType::TEXT_WHITESPACE, "·"),
            (TokenType::TEXT_WHITESPACE, "»   "),
            (TokenType::TEXT, "b"),
            (TokenType::TEXT_WHITESPACE, "¶\n"),
        ]
    );

    let gob = GobbleFilter::new(2);
    let gtoks = apply_filters(None, [(TokenType::TEXT, "  foo\n   bar\n")], &[&gob]).unwrap();
    assert_eq!(gtoks[0].1, "foo\n bar\n");

    let merge = TokenMergeFilter::new();
    let mtoks = apply_filters(None, [(TokenType::TEXT, "a"), (TokenType::TEXT, "b")], &[&merge]).unwrap();
    assert_eq!(mtoks.len(), 1);
    assert_eq!(mtoks[0].1, "ab");

    for &name in filter_names() {
        assert!(filter_by_name(name, &[]).is_ok(), "{name}");
    }
}

#[test]
fn all_fourteen_formatters() {
    assert_eq!(formatter_names().len(), 14);
    for &name in formatter_names() {
        assert!(formatter_by_name(name, &[]).is_ok(), "{name}");
    }
    assert!(formatter_for_filename("out.html", &[]).is_ok());
    assert!(formatter_for_filename("out.tex", &[]).is_ok());
    assert!(formatter_for_filename("out.rtf", &[]).is_ok());
    assert!(formatter_for_filename("out.svg", &[]).is_ok());
    assert!(formatter_for_filename("out.raw", &[]).is_ok());
    assert!(formatter_for_filename("out.txt", &[]).is_ok());

    let rust = lexer_by_name("rust").unwrap();
    let monokai = style_by_name("monokai").unwrap();
    let code = "fn f() {}\n// x\n";
    let pre = rust.preprocess(code);
    let toks: Vec<_> = rust.tokenize(&pre).collect();

    // 1. NullFormatter
    assert_eq!(format_string(toks.iter().copied(), &NullFormatter::new()), code);

    // 2. RawTokenFormatter + round-trip through RawTokenLexer
    let raw_out = format_string(toks.iter().copied(), &RawTokenFormatter::new());
    assert!(raw_out.starts_with("Token.Keyword\t'fn'\n"));
    let raw_lexer = pygments_rs::lexer_for_mimetype("application/x-pygments-tokens").unwrap();
    let rt_types: Vec<_> = raw_lexer.tokenize(&raw_out).map(|(t, _)| t).collect();
    assert_eq!(rt_types, toks.iter().map(|&(t, _)| t).collect::<Vec<_>>());

    // 3. TestcaseFormatter
    let tc = format_string(toks.iter().copied(), &TestcaseFormatter::new());
    assert!(tc.contains("def testNeedsName(lexer):"));
    assert!(tc.contains("(Token.Keyword, 'fn'),"));

    // 4. HtmlFormatter (table linenos, hl_lines, full, non-standard class, style_defs)
    let html_full = HtmlFormatter::new(monokai)
        .with_full()
        .with_title("Demo")
        .with_linenos(HtmlLineNumbers::Table)
        .with_hl_lines([1]);
    let ho = format_string(toks.iter().copied(), &html_full);
    assert!(ho.contains("<!DOCTYPE html"));
    assert!(ho.contains("<table class=\"highlighttable\">"));
    assert!(ho.contains("<span class=\"hll\">"));
    assert!(ho.contains("/* Keyword */"));
    let console = lexer_by_name("console").unwrap();
    let venv_html = highlight("(venv) $ ls\n", console, &HtmlFormatter::new(monokai));
    assert!(venv_html.contains("class=\"gp gp-VirtualEnv\""));

    // 5. TerminalFormatter
    let term =
        format_string(toks.iter().copied(), &TerminalFormatter::new().with_bg(TerminalBackground::Dark).with_linenos());
    assert!(term.contains("0001: "));
    assert!(term.contains("\x1b[94m"));

    // 6. Terminal256Formatter
    let t256 = format_string(toks.iter().copied(), &Terminal256Formatter::new(monokai));
    assert!(t256.contains("\x1b[38;5;"));

    // 7. TerminalTrueColorFormatter
    let t16m = format_string(toks.iter().copied(), &TerminalTrueColorFormatter::new(monokai));
    assert!(t16m.contains("\x1b[38;2;"));

    // 8. SvgFormatter
    let svg = format_string(toks.iter().copied(), &SvgFormatter::new(monokai).with_linenos());
    assert!(svg.starts_with("<?xml version=\"1.0\"?>\n"));
    assert!(svg.contains("&#160;"));
    assert!(svg.contains("<tspan"));

    // 9. LatexFormatter
    let tex_fmt = LatexFormatter::new(monokai).with_full().with_mathescape();
    let tex = format_string([(TokenType::KEYWORD, "fn"), (TokenType::COMMENT_SINGLE, "// $x_1$\n")], &tex_fmt);
    assert!(tex.contains(r"\begin{Verbatim}[commandchars=\\\{\}"));
    assert!(tex.contains(r"\PY{k}{fn}"));
    assert!(tex.contains("$x_1$"));

    // 10. RtfFormatter (including non-BMP surrogate pair)
    let rtf_fmt = RtfFormatter::new(monokai).with_linenos().with_hl_lines([1]);
    let rtf = format_string([(TokenType::KEYWORD, "fn"), (TokenType::LITERAL_STRING, "\"🦀\"\n")], &rtf_fmt);
    assert!(rtf.starts_with("{\\rtf1\\ansi\\uc0\\deff0"));
    assert!(rtf.contains("{\\highlight"));
    assert!(rtf.contains("{\\u55358}{\\u56704}"));

    // 11. BBCodeFormatter
    let bb = format_string(toks.iter().copied(), &BBCodeFormatter::new(monokai).with_codetag());
    assert!(bb.starts_with("[code]"));
    assert!(bb.contains("[/code]\n"));

    // 12. PangoMarkupFormatter
    let pango =
        format_string([(TokenType::KEYWORD, "fn"), (TokenType::TEXT, " <&>\n")], &PangoMarkupFormatter::new(monokai));
    assert!(pango.starts_with("<tt><span fgcolor=\"#"));
    assert!(pango.contains("&lt;&amp;>"));
    assert!(pango.ends_with("</tt>"));

    // 13. IrcFormatter
    let irc =
        format_string(toks.iter().copied(), &IrcFormatter::new().with_bg(TerminalBackground::Dark).with_linenos());
    assert!(irc.contains("\x03"));
    assert!(irc.starts_with("0001: "));

    // 14. GroffFormatter
    let groff = format_string(toks.iter().copied(), &GroffFormatter::new(monokai).with_linenos());
    assert!(groff.contains(".defcolor "));
    assert!(groff.contains(".nf\n\\f[CR]\n"));
    assert!(groff.ends_with("\n.fi"));
}

#[test]
fn pygmentize_cli() {
    let bin = env!("CARGO_BIN_EXE_pygmentize");

    let ver = Command::new(bin).arg("-V").output().unwrap();
    assert!(ver.status.success());
    assert!(String::from_utf8_lossy(&ver.stdout).starts_with("pygmentize (pygments-rs "));

    let guess_name = Command::new(bin).args(["-N", "main.rs"]).output().unwrap();
    assert!(guess_name.status.success());
    assert_eq!(String::from_utf8_lossy(&guess_name.stdout).trim(), "rust");

    let css = Command::new(bin).args(["-S", "monokai", "-f", "html", "-a", ".code"]).output().unwrap();
    assert!(css.status.success());
    let css_out = String::from_utf8_lossy(&css.stdout);
    assert!(css_out.contains(".code .hll {"));
    assert!(css_out.contains(".code .k {"));

    let mut child = Command::new(bin)
        .args(["-l", "rust", "-f", "html", "-F", "keywordcase:case=upper"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.as_mut().unwrap().write_all(b"fn x() {}\n").unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains(">FN<"));

    let bad = Command::new(bin).args(["-l", "no-such-lexer-xyz"]).output().unwrap();
    assert!(!bad.status.success());
}
