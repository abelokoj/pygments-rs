//! Expected values come from Pygments 2.19.2 `Lexer.analyse_text` (already clamped).

use crate::analyse::score;

#[test]
fn matches_pygments() {
    let cases: &[(&str, &str, f64)] = &[
        ("BrainfuckLexer", "++++[>+<-]", 0.0),
        ("CarbonLexer", "package Foo impl forall type Self observe library choice addr constraint", 0.9999999999999999),
        ("DiffLexer", "--- a\n+++ b\n", 0.9),
        ("DjangoLexer", "{% extends 'x' %}{{ y }}", 0.5),
        ("HtmlDjangoLexer", "<!DOCTYPE html>{% block a %}", 0.89),
        ("EasytrieveLexer", "* hdr\nPARM LINK(X)\nFILE F1\nJOB INPUT F1\n", 0.6),
        ("GroffLexer", ".TH FOO 1\n", 1.0),
        ("GroffLexer", ".ab x", 0.9),
        ("GroffLexer", ".ab", 0.0),
        ("IniLexer", "[sec]\na=1\n", 0.8),
        ("MatlabLexer", "function y = f(x)\n", 1.0),
        ("MatlabLexer", "% c\nx=1\n", 0.2),
        ("Perl6Lexer", "use v6;\nsay 1;\n", 1.0),
        ("Perl6Lexer", "class Foo { }\n", 0.05),
        ("PerlLexer", "my $x = 1;\n", 0.9),
        ("PhpLexer", "<?php echo 1; ?>", 0.3),
        ("HtmlPhpLexer", "<!DOCTYPE html><?php echo 1; ?>", 0.79),
        ("XmlPhpLexer", "<a><?php echo 1; ?></a>", 0.69),
        ("PythonLexer", "#!/usr/bin/env python3\n", 1.0),
        ("NumPyLexer", "import numpy as np\n", 1.0),
        ("RexxLexer", "/* hello */\nsay 'x'\nparse arg a\n", 0.21000000000000002),
        ("RstLexer", "Title\n=====\n", 0.5),
        ("SwigLexer", "%module foo\n", 0.98),
        ("SwigLexer", "%foo bar\n", 0.91),
        ("TransactSqlLexer", "SELECT [a] FROM [b]\nGO\n", 0.6),
        ("MySqlLexer", "SELECT `a` FROM `b`", 0.5),
        ("GoogleSqlLexer", "SELECT ABS(x) FROM t WHERE y", 0.003),
        ("WoWTocLexer", "## Interface: 100207", 0.8),
        ("Inform6Lexer", "origsource", 0.0),
        ("ObjectiveCLexer", "@interface Foo\n@end\n", 1.0),
        ("TurtleLexer", "  @prefix x: <y> .\n", 0.8),
        ("VelocityXmlLexer", "<a>#if($x) y #end</a>", 0.55),
        ("GenshiLexer", "<a py:if=\"x\">${y}</a>", 0.8400000000000001),
        ("AntlrJavaLexer", "grammar Foo;\n", 0.0),
        ("SmaliLexer", ".class public LFoo;\ninvoke-virtual {}\n", 0.8),
        ("ScdocLexer", "foo(1)\n*bold*", 0.51),
        ("SourcesListLexer", "  deb http://x y main\n", 1.0),
        ("CMakeLexer", "cmake_minimum_required(VERSION 3.10)\n", 0.8),
    ];
    for &(class, text, want) in cases {
        let got = f64::from(score(class, text));
        assert!((got - want).abs() < 1e-6, "{class} on {text:?}: got {got}, want {want}");
    }
}

#[test]
fn every_custom_owner_is_ported() {
    let json: serde_json::Value =
        serde_json::from_str(include_str!("../../../data/analyse.json")).expect("bundled analyse.json is valid");
    let owners = json.as_object().into_iter().flatten().filter_map(|(_, v)| v.get("custom")?.as_str());
    for owner in owners {
        assert!(super::MODULE.analyse.iter().any(|(k, _)| *k == owner), "{owner} not ported");
    }
}

#[test]
fn python_string_helpers() {
    use super::{head, py_splitlines, py_words};
    assert_eq!(py_splitlines("a\r\nb\rc\x0cd\n").collect::<Vec<_>>(), ["a", "b", "c", "d"]);
    assert_eq!(py_splitlines("").count(), 0);
    assert_eq!(py_words(" a\x1cb  c ").collect::<Vec<_>>(), ["a", "b", "c"]);
    assert_eq!(head("\u{e9}\u{e9}x", 2), "\u{e9}\u{e9}");
}
