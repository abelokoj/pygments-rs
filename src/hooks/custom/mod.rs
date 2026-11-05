//! Hand-written Pygments lexers (`Lexer` subclasses that override
//! `get_tokens_unprocessed`), ported one module per file.

use onig::{Regex, SearchOptions};

use super::Module;

mod delphi;
mod json;
mod make;
mod robot;
mod templates;
mod tnt;

pub(crate) static MODULE: Module = Module {
    callbacks: &[],
    filters: &[],
    custom: &[
        ("DelphiLexer", delphi::delphi),
        ("ErbLexer", templates::erb),
        ("GosuTemplateLexer", templates::gosu_template),
        ("JsonBareObjectLexer", json::json),
        ("JsonLdLexer", json::json_ld),
        ("JsonLexer", json::json),
        ("MakefileLexer", make::makefile),
        ("PortugolLexer", delphi::portugol),
        ("RobotFrameworkLexer", robot::robot),
        ("TNTLexer", tnt::tnt),
    ],
    analyse: &[],
};

/// Python's `str.isspace()` for one char.
fn py_space(c: char) -> bool {
    c.is_whitespace() || ('\x1c'..='\x1f').contains(&c)
}

/// Python's `str.strip()`.
fn py_strip(s: &str) -> &str {
    s.trim_matches(py_space)
}

/// Anchored match at `at` (Python `pattern.match(text, at)`); the match length.
fn match_at(re: &Regex, text: &str, at: usize) -> Option<usize> {
    re.match_with_options(text, at, SearchOptions::SEARCH_OPTION_NONE, None)
}

/// Python's `str.splitlines()`: `(start, content_end, next_start)` per line.
fn py_lines(text: &str) -> impl Iterator<Item = (usize, usize, usize)> + '_ {
    let mut pos = 0;
    std::iter::from_fn(move || {
        if pos >= text.len() {
            return None;
        }
        let start = pos;
        let rest = &text[pos..];
        for (i, c) in rest.char_indices() {
            let n = match c {
                '\r' if rest[i + 1..].starts_with('\n') => 2,
                '\n' | '\r' | '\x0b' | '\x0c' | '\x1c' | '\x1d' | '\x1e' | '\u{85}' | '\u{2028}' | '\u{2029}' => {
                    c.len_utf8()
                }
                _ => continue,
            };
            pos = start + i + n;
            return Some((start, start + i, pos));
        }
        pos = text.len();
        Some((start, text.len(), text.len()))
    })
}

#[cfg(test)]
fn lex(alias: &str, text: &str) -> Vec<(String, String)> {
    crate::lexer_by_name(alias)
        .expect("alias")
        .tokenize(text)
        .map(|(t, v)| (t.short_path().to_owned(), v.to_owned()))
        .collect()
}

/// Expected tokens from a `"Type|value"`-style list.
#[cfg(test)]
fn exp(items: &[(&str, &str)]) -> Vec<(String, String)> {
    items.iter().map(|(t, v)| ((*t).to_owned(), (*v).to_owned())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitlines_like_python() {
        let v: Vec<_> = py_lines("a\r\nb\x0cc").collect();
        assert_eq!(v, vec![(0, 1, 3), (3, 4, 5), (5, 6, 6)]);
        assert_eq!(py_lines("").count(), 0);
    }
}
