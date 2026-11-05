//! Golden tests against Pygments' own expected outputs.
//!
//! Set `PYGMENTS_TESTS` to the `tests/` directory of a Pygments checkout at
//! the tag in `PYGMENTS_VERSION` (CI does this). Each file under
//! `snippets/<alias>/` and `examplefiles/<alias>/` is lexed with the lexer
//! for `<alias>` and printed exactly like Pygments' `tests/conftest.py`.
//!
//! Every file listed in `tests/golden-baseline.txt` must pass; others are
//! reported. The report (pass rate per lexer) is written to
//! `target/golden-report.md`, and the passing list to
//! `target/golden-passing.txt` so the baseline can be refreshed.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use pygments_rs::{TokenType, lexer_by_name};

/// Python's `repr()` of a `str`.
fn py_repr(s: &str) -> String {
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
            c if !py_printable(c) => {
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

/// Approximation of Python's `str.isprintable()` for non-ASCII characters:
/// separators (other than space), format, control and private-use characters
/// are not printable.
fn py_printable(c: char) -> bool {
    let n = c as u32;
    !matches!(n,
        0x80..=0xa0 | 0xad | 0x600..=0x605 | 0x61c | 0x6dd | 0x70f | 0x1680 | 0x180e
        | 0x2000..=0x200f | 0x2028..=0x202f | 0x205f..=0x206f | 0x3000 | 0xd800..=0xf8ff
        | 0xfeff | 0xfff9..=0xfffb | 0xe0001 | 0xe0020..=0xe007f | 0xf0000..=0x10ffff)
}

/// Read like Python's text mode: UTF-8 with universal newlines.
fn read_text(p: &Path) -> Option<String> {
    let s = String::from_utf8(std::fs::read(p).ok()?).ok()?;
    Some(if s.contains('\r') { s.replace("\r\n", "\n").replace('\r', "\n") } else { s })
}

/// `LexerTestItem.runtest` + `_prettyprint_tokens`. `Err` if an `Error`
/// token appears where Pygments forbids it.
fn render(alias: &str, input: &str, allow_errors: bool) -> Result<String, String> {
    let lexer = lexer_by_name(alias).ok_or_else(|| format!("no lexer for alias {alias:?}"))?;
    let text = lexer.preprocess(input);
    let mut lines = Vec::new();
    for (ty, val) in lexer.tokenize(&text) {
        if ty == TokenType::ERROR && !allow_errors {
            return Err(format!("generated Error token at {}", py_repr(val)));
        }
        let r = py_repr(val);
        let pad = 13usize.saturating_sub(r.chars().count());
        lines.push(format!("{r}{} {}", " ".repeat(pad), ty.short_path()));
        if val.ends_with('\n') {
            lines.push(String::new());
        }
    }
    Ok(lines.join("\n").trim_end_matches('\n').to_owned() + "\n")
}

struct Case {
    id: String,
    alias: String,
    input: String,
    expected: String,
    allow_errors: bool,
}

fn collect(root: &Path) -> Vec<Case> {
    let mut cases = Vec::new();
    for kind in ["snippets", "examplefiles"] {
        let Ok(dirs) = std::fs::read_dir(root.join(kind)) else { continue };
        let mut dirs: Vec<PathBuf> = dirs.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.is_dir()).collect();
        dirs.sort();
        for dir in dirs {
            let alias = dir.file_name().unwrap().to_string_lossy().into_owned();
            let mut files: Vec<PathBuf> =
                std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok().map(|e| e.path())).collect();
            files.sort();
            for f in files {
                let name = f.file_name().unwrap().to_string_lossy().into_owned();
                let id = format!("{kind}/{alias}/{name}");
                if kind == "snippets" {
                    if !name.ends_with(".txt") {
                        continue;
                    }
                    let Some(content) = read_text(&f) else { continue };
                    let (content, expected) = match content.split_once("\n---tokens---\n") {
                        Some((c, e)) => (c.to_owned(), e.to_owned()),
                        None => (content, String::new()),
                    };
                    let content = if content.starts_with("---input---\n") { format!("\n{content}") } else { content };
                    let mut input =
                        content.rsplit_once("\n---input---\n").map_or(content.clone(), |(_, i)| i.to_owned());
                    if !input.ends_with('\n') {
                        input.push('\n');
                    }
                    cases.push(Case { id, alias: alias.clone(), input, expected, allow_errors: true });
                } else {
                    if name.ends_with(".output") || !f.is_file() {
                        continue;
                    }
                    let Some(input) = read_text(&f) else { continue };
                    let expected = read_text(&f.with_file_name(format!("{name}.output"))).unwrap_or_default();
                    cases.push(Case { id, alias: alias.clone(), input, expected, allow_errors: false });
                }
            }
        }
    }
    cases
}

fn baseline() -> BTreeSet<String> {
    include_str!("golden-baseline.txt")
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

#[test]
fn golden() {
    let Some(root) = std::env::var_os("PYGMENTS_TESTS").map(PathBuf::from) else {
        eprintln!("PYGMENTS_TESTS not set; skipping golden tests");
        return;
    };
    let cases = collect(&root);
    assert!(!cases.is_empty(), "no test files under {}", root.display());

    let mut per_alias: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut passing = Vec::new();
    let mut failures = Vec::new();
    for case in &cases {
        let result = std::panic::catch_unwind(|| render(&case.alias, &case.input, case.allow_errors));
        let ok = match result {
            Ok(Ok(actual)) => actual == case.expected,
            Ok(Err(_)) | Err(_) => false,
        };
        let e = per_alias.entry(case.alias.clone()).or_default();
        e.1 += 1;
        if ok {
            e.0 += 1;
            passing.push(case.id.clone());
        } else {
            failures.push(case.id.clone());
        }
    }

    let total_pass = passing.len();
    let mut report = format!(
        "# Golden test report\n\n{total_pass}/{} files match Pygments {} exactly.\n\n| lexer alias | passed | total | rate |\n|---|---:|---:|---:|\n",
        cases.len(),
        pygments_rs::PYGMENTS_VERSION,
    );
    let full = per_alias.values().filter(|(p, t)| p == t).count();
    for (alias, (p, t)) in &per_alias {
        let _ = writeln!(report, "| {alias} | {p} | {t} | {:.0}% |", 100.0 * *p as f64 / *t as f64);
    }
    let _ = writeln!(report, "\n{full}/{} lexer aliases pass all their files.", per_alias.len());
    let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
    let _ = std::fs::create_dir_all(&target);
    let _ = std::fs::write(target.join("golden-report.md"), &report);
    let _ = std::fs::write(target.join("golden-passing.txt"), passing.join("\n") + "\n");
    eprintln!("{report}");

    let failures: BTreeSet<String> = failures.into_iter().collect();
    let baseline = baseline();
    let regressions: Vec<&String> = baseline.iter().filter(|id| failures.contains(*id)).collect();
    assert!(regressions.is_empty(), "files in golden-baseline.txt now fail:\n{regressions:#?}");
}

#[test]
fn repr_matches_python() {
    assert_eq!(py_repr("a"), "'a'");
    assert_eq!(py_repr("it's"), "\"it's\"");
    assert_eq!(py_repr("'\""), "'\\'\"'");
    assert_eq!(py_repr("\n\t\\"), "'\\n\\t\\\\'");
    assert_eq!(py_repr("\x1b"), "'\\x1b'");
    assert_eq!(py_repr("\u{a0}"), "'\\xa0'");
    assert_eq!(py_repr("é"), "'é'");
}
