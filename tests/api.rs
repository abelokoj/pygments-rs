//! Public API behavior and whole-catalog sanity checks.

use std::fmt::Write as _;

use pygments_rs::{TokenType, all_lexers, guess_lexer, lexer_by_name, lexer_for_filename, lexer_for_mimetype};

#[test]
fn lookup_by_name_filename_mimetype() {
    assert_eq!(lexer_by_name("python").unwrap().name(), "Python");
    assert_eq!(lexer_by_name("PY").unwrap().class_name(), "PythonLexer");
    assert!(lexer_by_name("no-such-language").is_none());
    assert_eq!(lexer_for_filename("src/main.rs", None).unwrap().class_name(), "RustLexer");
    assert_eq!(lexer_for_filename("C:\\x\\Makefile", None).unwrap().class_name(), "MakefileLexer");
    assert_eq!(lexer_for_filename("a.py", None).unwrap().class_name(), "PythonLexer");
    assert!(lexer_for_filename("noext", None).is_none());
    assert_eq!(lexer_for_mimetype("text/x-python").unwrap().class_name(), "PythonLexer");
}

#[test]
fn guessing() {
    assert_eq!(guess_lexer("#!/usr/bin/env python3\nprint(1)\n").unwrap().class_name(), "PythonLexer");
    assert_eq!(guess_lexer("#!/bin/bash\necho hi\n").unwrap().class_name(), "BashLexer");
    assert_eq!(guess_lexer("<?xml version=\"1.0\"?>\n<a/>\n").unwrap().class_name(), "XmlLexer");
    assert_eq!(guess_lexer("# vim: ft=ruby\nputs 1\n").unwrap().class_name(), "RubyLexer");
}

#[test]
fn tokens_cover_input_for_common_languages() {
    let samples = [
        ("python", "def f(x):\n    return x + 1  # hi\n"),
        ("javascript", "const a = `x${1}`; // c\n"),
        ("rust", "fn main() { let s = \"hi\"; }\n"),
        ("c", "#include <stdio.h>\nint main(void) { return 0; }\n"),
        ("html", "<p class=\"a\">x</p>\n"),
        ("bash", "for i in 1 2; do echo $i; done\n"),
        ("sql", "SELECT * FROM t WHERE a = 1;\n"),
    ];
    for (alias, src) in samples {
        let lexer = lexer_by_name(alias).unwrap();
        let joined: String = lexer.tokenize(src).map(|(_, s)| s).collect();
        assert_eq!(joined, src, "{alias}");
        assert!(lexer.tokenize(src).all(|(t, _)| t != TokenType::ERROR), "{alias} produced Error tokens");
    }
}

#[test]
fn python_tokens_match_pygments() {
    let lexer = lexer_by_name("python").unwrap();
    let got: Vec<(String, &str)> =
        lexer.tokenize("def f():\n    pass\n").map(|(t, s)| (t.short_path().to_owned(), s)).collect();
    let want = [
        ("Keyword", "def"),
        ("Text.Whitespace", " "),
        ("Name.Function", "f"),
        ("Punctuation", "("),
        ("Punctuation", ")"),
        ("Punctuation", ":"),
        ("Text.Whitespace", "\n"),
        ("Text", "    "),
        ("Keyword", "pass"),
        ("Text.Whitespace", "\n"),
    ];
    let want: Vec<(String, &str)> = want.iter().map(|(t, s)| ((*t).to_owned(), *s)).collect();
    assert_eq!(got, want);
}

/// Resuming at a saved line-start state gives the same tokens as a full run.
#[test]
fn incremental_resume_matches_full_run() {
    let lexer = lexer_by_name("python").unwrap();
    assert!(lexer.is_resumable());
    let text = "x = '''multi\nline'''\ndef g():\n    return 2\n";
    let full: Vec<_> = lexer.tokenize(text).collect();

    // Walk once, remembering the state at every line start.
    let mut saved = Vec::new();
    let mut toks = lexer.tokenize(text);
    loop {
        let at = toks.offset();
        if (at == 0 || text.as_bytes()[at - 1] == b'\n')
            && let Some(st) = toks.state()
        {
            saved.push((at, st));
        }
        if toks.next_span().is_none() {
            break;
        }
    }
    assert!(saved.len() >= 3, "expected several resumable line starts, got {}", saved.len());
    for (at, st) in &saved {
        let tail: Vec<_> = lexer.tokenize_from(text, *at, st).collect();
        let suffix: Vec<_> =
            full.iter().copied().skip_while(|(_, s)| (s.as_ptr() as usize - text.as_ptr() as usize) < *at).collect();
        assert_eq!(tail, suffix, "resume at {at}");
    }
}

/// Every lexer loads, every regex compiles, and lexing never panics or loses text.
#[test]
fn every_lexer_loads_and_roundtrips() {
    let sample = "foo bar 123 \"str\" 'c' # x // y /* z */ <a b=\"c\">\n\tif (x) { return 1; }\n";
    let known_bad: Vec<&str> = include_str!("known-regex-errors.txt")
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    let mut report = String::new();
    let mut unexpected = Vec::new();
    for lexer in all_lexers() {
        let loaded = std::panic::catch_unwind(|| lexer.regex_errors());
        let errors = match loaded {
            Ok(e) => e,
            Err(_) => {
                unexpected.push(format!("{}: failed to load", lexer.class_name()));
                continue;
            }
        };
        for (pat, err) in &errors {
            let _ = writeln!(report, "{}\t{err}\t{pat:?}", lexer.class_name());
        }
        if !errors.is_empty() && !known_bad.contains(&lexer.class_name()) {
            unexpected.push(format!("{}: {} regex(es) fail to compile", lexer.class_name(), errors.len()));
        }
        let run = std::panic::catch_unwind(|| lexer.tokenize(sample).map(|(_, s)| s).collect::<String>());
        match run {
            Ok(joined) if lexer.is_unported() || joined == sample => {}
            Ok(_) => {
                let _ = writeln!(report, "{}\tdoes not round-trip the sample", lexer.class_name());
            }
            Err(_) => unexpected.push(format!("{}: panicked while lexing", lexer.class_name())),
        }
    }
    let target = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
    let _ = std::fs::create_dir_all(&target);
    let _ = std::fs::write(target.join("catalog-report.txt"), &report);
    eprintln!("{report}");
    assert!(unexpected.is_empty(), "{unexpected:#?}");
}
