//! Finding lexers: by alias, file name, MIME type, or by guessing from the
//! text. Ports `pygments.lexers.get_lexer_by_name` and friends.

use std::collections::HashMap;
use std::sync::{LazyLock, OnceLock};

use crate::generated::registry::LEXERS;
use crate::lexer::{Lexer, LexerImpl};

/// Static metadata for one lexer, generated from Pygments' `_mapping.py`.
pub(crate) struct LexerInfo {
    pub class: &'static str,
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub filenames: &'static [&'static str],
    pub mimetypes: &'static [&'static str],
    pub priority: f64,
    pub stripnl: bool,
    pub stripall: bool,
    pub ensurenl: bool,
    pub tabsize: usize,
    pub data: &'static str,
}

static BUILT: LazyLock<Box<[OnceLock<LexerImpl>]>> = LazyLock::new(|| LEXERS.iter().map(|_| OnceLock::new()).collect());

pub(crate) fn info(i: u16) -> &'static LexerInfo {
    &LEXERS[usize::from(i)]
}

pub(crate) fn get(i: u16) -> &'static LexerImpl {
    let i = usize::from(i);
    BUILT[i].get_or_init(|| LexerImpl::build(&LEXERS[i]))
}

/// Lexer by Pygments class name (`LEXERS` is sorted by it).
pub(crate) fn lexer_by_class(class: &str) -> Option<Lexer> {
    LEXERS.binary_search_by(|l| l.class.cmp(class)).ok().map(|i| Lexer(i as u16))
}

pub(crate) fn by_class(class: &str) -> Option<&'static LexerImpl> {
    lexer_by_class(class).map(Lexer::imp)
}

/// All lexers, sorted by class name.
pub fn all_lexers() -> impl ExactSizeIterator<Item = Lexer> {
    (0..LEXERS.len() as u16).map(Lexer)
}

/// Lexer by alias (`"python"`, `"rs"`, ...), case-insensitively.
/// Like Pygments, the first lexer in class-name order that lists the alias wins.
pub fn lexer_by_name(alias: &str) -> Option<Lexer> {
    static BY_ALIAS: LazyLock<HashMap<&'static str, u16>> = LazyLock::new(|| {
        let mut m = HashMap::new();
        for (i, l) in LEXERS.iter().enumerate() {
            for a in l.aliases {
                m.entry(*a).or_insert(i as u16);
            }
        }
        m
    });
    BY_ALIAS.get(alias.to_lowercase().as_str()).copied().map(Lexer)
}

/// Lexer by MIME type (`"text/x-python"`), exact match.
pub fn lexer_for_mimetype(mime: &str) -> Option<Lexer> {
    LEXERS.iter().position(|l| l.mimetypes.contains(&mime)).map(|i| Lexer(i as u16))
}

/// Lexer for a file name such as `"src/main.rs"` (only the last path
/// component is used). When several lexers match, `code` (the file's
/// contents, if available) is scored with each lexer's heuristics;
/// otherwise the lexer priority decides. Exact names like `Makefile` beat
/// wildcard patterns. Port of `find_lexer_class_for_filename`.
pub fn lexer_for_filename(filename: &str, code: Option<&str>) -> Option<Lexer> {
    let base = filename.rsplit(['/', '\\']).next().unwrap_or(filename);
    let mut best: Option<(f64, &str, Lexer)> = None;
    for lexer in all_lexers() {
        for glob in lexer.filenames() {
            if !fnmatch(base, glob) {
                continue;
            }
            let bonus = if glob.contains('*') { 0.0 } else { 0.5 };
            let rating = match code {
                Some(c) if !c.is_empty() => f64::from(lexer.analyse_text(c)) + bonus,
                _ => registry_priority(lexer) + bonus,
            };
            let key = (rating, lexer.class_name());
            // Python sorts by (rating, class name) and takes the last.
            if best.is_none_or(|(r, c, _)| (key.0, key.1) >= (r, c)) {
                best = Some((key.0, key.1, lexer));
            }
        }
    }
    best.map(|(_, _, l)| l)
}

fn registry_priority(lexer: Lexer) -> f64 {
    info(lexer.0).priority
}

/// Guess the language of `text`: a Vim/Emacs-style modeline wins, then the
/// lexer whose heuristics score highest. `None` if nothing scores above 0.
/// Port of `pygments.lexers.guess_lexer`.
pub fn guess_lexer(text: &str) -> Option<Lexer> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if let Some(l) = crate::analyse::modeline_filetype(text).and_then(lexer_by_name) {
        return Some(l);
    }
    let mut best: Option<(f32, Lexer)> = None;
    for lexer in all_lexers() {
        let rv = lexer.analyse_text(text);
        if rv >= 1.0 {
            return Some(lexer);
        }
        if rv > best.map_or(0.0, |b| b.0) {
            best = Some((rv, lexer));
        }
    }
    best.map(|b| b.1)
}

/// Python's `fnmatch` as Pygments uses it: case-sensitive, `*` and `?`
/// match any character including `/`, `[seq]` and `[!seq]` match sets, an
/// unclosed `[` is literal.
pub(crate) fn fnmatch(name: &str, pat: &str) -> bool {
    let name: Vec<char> = name.chars().collect();
    let pat: Vec<char> = pat.chars().collect();
    fn class(pat: &[char], c: char) -> Option<(bool, usize)> {
        // pat[0] == '['; returns (matches, length of the class in pat).
        let mut i = 1;
        let negate = pat.get(i) == Some(&'!');
        if negate {
            i += 1;
        }
        let start = i;
        let mut hit = false;
        while i < pat.len() && (pat[i] != ']' || i == start) {
            if i + 2 < pat.len() && pat[i + 1] == '-' && pat[i + 2] != ']' {
                hit |= pat[i] <= c && c <= pat[i + 2];
                i += 3;
            } else {
                hit |= pat[i] == c;
                i += 1;
            }
        }
        (i < pat.len()).then_some((hit != negate, i + 1))
    }
    fn go(n: &[char], p: &[char]) -> bool {
        match p.first() {
            None => n.is_empty(),
            Some('*') => (0..=n.len()).any(|k| go(&n[k..], &p[1..])),
            Some('?') => !n.is_empty() && go(&n[1..], &p[1..]),
            Some('[') => match (n.first(), class(p, n.first().copied().unwrap_or('\0'))) {
                (Some(_), Some((true, len))) => go(&n[1..], &p[len..]),
                (_, Some((false, _))) | (None, Some(_)) => false,
                (_, None) => n.first() == Some(&'[') && go(&n[1..], &p[1..]),
            },
            Some(c) => n.first() == Some(c) && go(&n[1..], &p[1..]),
        }
    }
    go(&name, &pat)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnmatch_like_python() {
        assert!(fnmatch("main.rs", "*.rs"));
        assert!(!fnmatch("main.rs", "*.py"));
        assert!(fnmatch("Makefile", "Makefile"));
        assert!(!fnmatch("makefile", "Makefile"));
        assert!(fnmatch("a.php5", "*.php[345]"));
        assert!(!fnmatch("a.php6", "*.php[345]"));
        assert!(fnmatch("a.x", "*.[!y]"));
        assert!(fnmatch("a[b", "a[b"));
        assert!(fnmatch("x.c", "?.c"));
        assert!(fnmatch("Dockerfile.dev", "Dockerfile.*"));
    }

    #[test]
    fn sorted_by_class() {
        assert!(LEXERS.windows(2).all(|w| w[0].class < w[1].class));
    }
}
