//! Translate Python `re` patterns for Oniguruma's Python syntax.
//!
//! Pygments lexers are authored for CPython's `re` module, whose defaults differ
//! from Oniguruma (`Syntax::python()`) in two places that affect lexer parity:
//!
//! 1. Leading inline flags such as `(?i)a|b` apply to the entire pattern in
//!    Python, whereas Oniguruma's `Syntax::python()` enables
//!    `ONIG_SYN_ISOLATED_OPTION_CONTINUE_BRANCH` and scopes them only to the
//!    first `|` branch. Hoisting leading `(?[imsx]+)` groups into `RegexOptions`
//!    restores Python's alternation semantics.
//! 2. Oniguruma's Unicode `\w` (`CR_Word`) includes `Mc`/`Me`/`Mn`/`Pc`/`So`/`Cn`
//!    characters (e.g. `‿` U+203F in BQN or Tamil combining marks in Ezhil) and
//!    omits `No`, while Python 3 `\w` is strictly `[\p{L}\p{N}_]`. Expanding
//!    `\w`, `\W`, `\b`, and `\B` matches Python across all Unicode scalar values,
//!    and expands `(?<=\b)`/`(?=\b)` lookarounds that Oniguruma rejects directly.

use onig::{Regex, RegexOptions, Syntax};

const WORD_INNER: &str = r"\p{L}\p{N}_";
const WORD: &str = r"(?-i:[\p{L}\p{N}_])";
const NOT_WORD: &str = r"(?-i:[^\p{L}\p{N}_])";
const BOUNDARY: &str =
    r"(?:(?<=(?-i:[\p{L}\p{N}_]))(?!(?-i:[\p{L}\p{N}_]))|(?<!(?-i:[\p{L}\p{N}_]))(?=(?-i:[\p{L}\p{N}_])))";
const NOT_BOUNDARY: &str =
    r"(?:(?<=(?-i:[\p{L}\p{N}_]))(?=(?-i:[\p{L}\p{N}_]))|(?<!(?-i:[\p{L}\p{N}_]))(?!(?-i:[\p{L}\p{N}_])))";

/// Map Python `re` flag letters (`"imsx"`) to Oniguruma [`RegexOptions`].
pub(crate) fn regex_options(flags: &str) -> RegexOptions {
    let mut opts = RegexOptions::REGEX_OPTION_CAPTURE_GROUP;
    for c in flags.chars() {
        opts |= match c {
            'i' => RegexOptions::REGEX_OPTION_IGNORECASE,
            // Python MULTILINE: ^/$ at line breaks. Onig's Python syntax
            // defaults to SINGLELINE, so negate it.
            'm' => RegexOptions::REGEX_OPTION_NEGATE_SINGLELINE,
            // Python DOTALL is Onig's (Ruby-style) MULTILINE.
            's' => RegexOptions::REGEX_OPTION_MULTILINE,
            'x' => RegexOptions::REGEX_OPTION_EXTEND,
            _ => RegexOptions::REGEX_OPTION_NONE,
        };
    }
    opts
}

fn flag_bits(s: &str) -> Option<(RegexOptions, bool)> {
    if s.is_empty() {
        return None;
    }
    let mut opts = RegexOptions::REGEX_OPTION_NONE;
    let mut verbose = false;
    for b in s.bytes() {
        match b {
            b'i' => opts |= RegexOptions::REGEX_OPTION_IGNORECASE,
            b'm' => opts |= RegexOptions::REGEX_OPTION_NEGATE_SINGLELINE,
            b's' => opts |= RegexOptions::REGEX_OPTION_MULTILINE,
            b'x' => {
                opts |= RegexOptions::REGEX_OPTION_EXTEND;
                verbose = true;
            }
            _ => return None,
        }
    }
    Some((opts, verbose))
}

fn hoist_leading_flags<'a>(mut pat: &'a str, opts: &mut RegexOptions) -> &'a str {
    while let Some(after) = pat.strip_prefix("(?") {
        let Some(end) = after.find(')') else { break };
        if let Some((extra, _)) = flag_bits(&after[..end]) {
            *opts |= extra;
            pat = &after[end + 1..];
        } else {
            break;
        }
    }
    pat
}

fn parse_inline_verbose(group: &str, current: bool) -> Option<(bool, bool)> {
    // Handles `flags)` or `flags:`: return `(inner_verbose, is_scoped_group)`.
    let (body, scoped) = if let Some(b) = group.strip_suffix(':') {
        (b, true)
    } else {
        let b = group.strip_suffix(')')?;
        (b, false)
    };
    let (pos, neg) = body.split_once('-').unwrap_or((body, ""));
    let is_flag = |b: u8| matches!(b, b'a' | b'i' | b'L' | b'm' | b's' | b'u' | b'x');
    if !pos.bytes().all(is_flag) || !neg.bytes().all(is_flag) || (pos.is_empty() && neg.is_empty()) {
        return None;
    }
    let mut v = current;
    if pos.contains('x') {
        v = true;
    }
    if neg.contains('x') {
        v = false;
    }
    Some((v, scoped))
}

fn scan_char_class(s: &str) -> Option<(bool, &str, usize)> {
    // `s` starts immediately after `[`.
    let bytes = s.as_bytes();
    let mut i = 0;
    let negated = bytes.first() == Some(&b'^');
    if negated {
        i += 1;
    }
    let body_start = i;
    if bytes.get(i) == Some(&b']') {
        i += 1;
    }
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                i += 1;
                if let Some(ch) = s[i..].chars().next() {
                    i += ch.len_utf8();
                }
            }
            b']' => {
                return Some((negated, &s[body_start..i], i + 1));
            }
            _ => {
                let ch = s[i..].chars().next()?;
                i += ch.len_utf8();
            }
        }
    }
    None
}

fn push_escape(esc: char, rest: &str, out: &mut String) -> usize {
    if esc == 'x'
        && let Some(hex) = rest.get(..2)
        && let Ok(byte) = u8::from_str_radix(hex, 16)
        && byte >= 0x80
    {
        // In Python `re` on `str`, `\x80`-`\xff` denotes U+0080-U+00FF, while Oniguruma treats
        // `\xHH` >= 0x80 as a raw byte that never matches UTF-8 multi-byte sequences.
        out.push_str("\\u00");
        out.push_str(hex);
        return 2;
    }
    out.push('\\');
    out.push(esc);
    0
}

fn translate_class(negated: bool, body: &str, out: &mut String) {
    // Fast-path exact rewrites for Python `\W` character-class idioms.
    match (negated, body) {
        (false, r"\w\W" | r"\W\w") => {
            out.push_str(r"[\s\S]");
            return;
        }
        (false, r"\W") => {
            out.push_str(NOT_WORD);
            return;
        }
        (true, r"\W") => {
            out.push_str(WORD);
            return;
        }
        (true, r"\W\d" | r"\d\W") => {
            out.push_str(r"[\p{L}\p{Nl}\p{No}_]");
            return;
        }
        (false, r"\W\d" | r"\d\W") => {
            out.push_str(r"[^\p{L}\p{Nl}\p{No}_]");
            return;
        }
        (true, r"\W_" | r"_\W") => {
            out.push_str(r"[\p{L}\p{N}]");
            return;
        }
        (false, r"\W_" | r"_\W") => {
            out.push_str(r"[^\p{L}\p{N}]");
            return;
        }
        _ => {}
    }

    let mut has_not_word = false;
    let mut rem = String::with_capacity(body.len());
    let mut bi = 0;
    while let Some(ch) = body[bi..].chars().next() {
        if bi == 0 && ch == ']' {
            // Preserve a leading literal `]` even if `\W` precedes or splits the class.
            rem.push_str(r"\]");
            bi += 1;
            continue;
        }
        if ch == '\\' {
            bi += 1;
            if let Some(esc) = body[bi..].chars().next() {
                bi += esc.len_utf8();
                match esc {
                    'w' => rem.push_str(WORD_INNER),
                    'W' => has_not_word = true,
                    other => bi += push_escape(other, &body[bi..], &mut rem),
                }
            } else {
                rem.push('\\');
            }
        } else {
            rem.push(ch);
            bi += ch.len_utf8();
        }
    }

    if has_not_word {
        if rem.is_empty() {
            out.push_str(if negated { WORD } else { NOT_WORD });
        } else if negated {
            out.push_str("(?:(?![");
            out.push_str(&rem);
            out.push_str("])");
            out.push_str(WORD);
            out.push(')');
        } else {
            out.push_str("(?:");
            out.push_str(NOT_WORD);
            out.push_str("|[");
            out.push_str(&rem);
            out.push_str("])");
        }
    } else {
        out.push('[');
        if negated {
            out.push('^');
        }
        out.push_str(&rem);
        out.push(']');
    }
}

/// Translate a Python `re` pattern and flag letters (`"imsx"`) for Oniguruma.
pub(crate) fn translate(pattern: &str, flags: &str) -> (String, RegexOptions) {
    let mut opts = regex_options(flags);
    let body = hoist_leading_flags(pattern, &mut opts);

    let mut out = String::with_capacity(body.len() + 16);
    let mut verbose_stack = vec![opts.contains(RegexOptions::REGEX_OPTION_EXTEND)];
    let mut i = 0;

    while i < body.len() {
        let rest = &body[i..];
        let verbose = *verbose_stack.last().unwrap_or(&false);

        if verbose && rest.starts_with('#') {
            let line_end = rest.find('\n').map_or(body.len(), |n| i + n + 1);
            out.push_str(&body[i..line_end]);
            i = line_end;
            continue;
        }

        if rest.starts_with(r"(?<=\b)") || rest.starts_with(r"(?=\b)") {
            let adv = if rest.starts_with(r"(?<=\b)") { 7 } else { 6 };
            out.push_str(BOUNDARY);
            i += adv;
            continue;
        }
        if rest.starts_with(r"(?<!\b)") || rest.starts_with(r"(?!\b)") {
            let adv = if rest.starts_with(r"(?<!\b)") { 7 } else { 6 };
            out.push_str(NOT_BOUNDARY);
            i += adv;
            continue;
        }

        let Some(ch) = rest.chars().next() else { break };
        match ch {
            '\\' => {
                i += 1;
                if let Some(esc) = body[i..].chars().next() {
                    i += esc.len_utf8();
                    match esc {
                        'w' => out.push_str(WORD),
                        'W' => out.push_str(NOT_WORD),
                        'b' => out.push_str(BOUNDARY),
                        'B' => out.push_str(NOT_BOUNDARY),
                        other => i += push_escape(other, &body[i..], &mut out),
                    }
                } else {
                    out.push('\\');
                }
            }
            '[' => {
                if let Some((negated, class_body, consumed)) = scan_char_class(&body[i + 1..]) {
                    translate_class(negated, class_body, &mut out);
                    i += 1 + consumed;
                } else {
                    out.push('[');
                    i += 1;
                }
            }
            '(' => {
                if let Some(after) = rest.strip_prefix("(?")
                    && let Some(end) = after.find([':', ')'])
                {
                    let candidate = &after[..=end];
                    if let Some((new_v, scoped)) = parse_inline_verbose(candidate, verbose) {
                        if scoped {
                            verbose_stack.push(new_v);
                        } else if let Some(top) = verbose_stack.last_mut() {
                            *top = new_v;
                        }
                        out.push_str("(?");
                        out.push_str(candidate);
                        i += 2 + candidate.len();
                        continue;
                    }
                }
                verbose_stack.push(verbose);
                out.push('(');
                i += 1;
            }
            ')' => {
                if verbose_stack.len() > 1 {
                    verbose_stack.pop();
                }
                out.push(')');
                i += 1;
            }
            other => {
                out.push(other);
                i += other.len_utf8();
            }
        }
    }

    (out, opts)
}

/// Compile a Python-syntax pattern with Python `re` flag letters (`"imsx"`).
pub(crate) fn compile(pattern: &str, flags: &str) -> Result<Regex, onig::Error> {
    let (translated, opts) = translate(pattern, flags);
    Regex::with_options(&translated, opts, Syntax::python())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_match(pat: &str, flags: &str, s: &str) -> bool {
        let re = compile(pat, flags).expect("compile");
        re.match_with_options(s, 0, onig::SearchOptions::SEARCH_OPTION_NONE, None) == Some(s.len())
    }

    #[test]
    fn leading_flags_apply_to_all_branches() {
        assert!(is_match(r"(?i)foo|bar", "", "BAR"));
        assert!(is_match(r"(?is)a.b|c.d", "", "C\nD"));
        assert!(is_match(r"(?m)(?x)^ foo $ # comment with [\w", "", "foo"));
    }

    #[test]
    fn python_unicode_word_excludes_onig_extras() {
        // U+203F (Pc, BQN undertie) and U+0BC8 (Mc, Tamil vowel sign) are in Onig \w, not Python \w.
        assert!(!is_match(r"\w+", "", "‿"));
        assert!(!is_match(r"\w+", "", "\u{0bc8}"));
        assert!(is_match(r"\w+", "", "abc_123é"));
        assert!(is_match(r"\W", "", "‿"));
        assert!(!is_match(r"a\bb", "", "a"));
    }

    #[test]
    fn boundaries_lookbehinds_and_classes() {
        let re = compile(r"(?<=\b)foo(?=\b)", "").unwrap();
        assert_eq!(re.find("foo bar"), Some((0, 3)));
        assert_eq!(re.find("afoo"), None);

        // Inside [...], \b stays backspace, never a word boundary.
        assert!(is_match(r"[\b]", "", "\x08"));
        assert!(!is_match(r"[\b]", "", "a"));

        assert!(is_match(r"[\w\W]", "", "\n"));
        assert!(is_match(r"[^\W\d]+", "", "foo_baré"));
        assert!(!is_match(r"[^\W\d]+", "", "123"));
        assert!(is_match(r"[\W0-9_]+", "", "-1_‿"));
        assert!(!is_match(r"[\W0-9_]+", "", "abc"));
    }

    #[test]
    fn verbose_comments_ignore_brackets() {
        let pat = r"(?x)
            \w+  # matches [unclosed \w comment
            =
            \d+
        ";
        assert!(is_match(pat, "", "abc=42"));
    }

    #[test]
    fn high_hex_escapes_and_case_insensitive_lookbehind() {
        assert!(is_match(r"[^\x85]+", "", "abc"));
        assert!(!is_match(r"[^\x85]+", "", "a\u{0085}b"));
        assert!(is_match(r"\xa0+", "", "\u{00a0}\u{00a0}"));
        let re = compile(r"(?<=\w)\[", "i").unwrap();
        assert_eq!(re.find("foo[1]"), Some((3, 4)));
    }
}
