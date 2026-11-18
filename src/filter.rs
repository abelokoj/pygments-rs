//! Token stream post-processing filters, ported from `pygments.filters`.

use std::borrow::Cow;
use std::collections::HashSet;
use std::fmt;

use onig::Regex;

use crate::engine::compile;
use crate::generated::symbols::{ISABELLE_SYMBOLS, LATEX_SYMBOLS};
use crate::lexer::{Lexer, py_whitespace};
use crate::token::TokenType;

/// One token produced by a filter stream.
pub type CowToken<'a> = (TokenType, Cow<'a, str>);

/// Error raised by [`RaiseOnErrorTokenFilter`] when an [`TokenType::ERROR`]
/// token appears in a stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErrorToken(pub String);

impl fmt::Display for ErrorToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "error token in input: {:?}", self.0)
    }
}

impl std::error::Error for ErrorToken {}

/// Error returned when looking up or constructing a filter by name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilterError(pub String);

impl fmt::Display for FilterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for FilterError {}

/// Post-processor that transforms a stream of tokens before formatting.
pub trait Filter: Send + Sync {
    /// Rewrite or validate `stream`, optionally inspecting `lexer`.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorToken`] if [`RaiseOnErrorTokenFilter`] encounters an
    /// error token in `stream`.
    fn filter<'a>(
        &self,
        lexer: Option<Lexer>,
        stream: Box<dyn Iterator<Item = CowToken<'a>> + 'a>,
    ) -> Result<Vec<CowToken<'a>>, ErrorToken>;
}

/// Apply `filters` in sequence to `tokens`.
///
/// # Errors
///
/// Propagates any [`ErrorToken`] raised by a filter in `filters`.
pub fn apply_filters<'a, I, S>(
    lexer: Option<Lexer>,
    tokens: I,
    filters: &[&dyn Filter],
) -> Result<Vec<CowToken<'a>>, ErrorToken>
where
    I: IntoIterator<Item = (TokenType, S)>,
    S: Into<Cow<'a, str>>,
{
    let mut current: Vec<CowToken<'a>> = tokens.into_iter().map(|(ty, s)| (ty, s.into())).collect();
    for &f in filters {
        current = f.filter(lexer, Box::new(current.into_iter()))?;
    }
    Ok(current)
}

fn slice_cow<'a>(cow: &Cow<'a, str>, start: usize, end: usize) -> Cow<'a, str> {
    match cow {
        Cow::Borrowed(s) => Cow::Borrowed(&s[start..end]),
        Cow::Owned(s) => Cow::Owned(s[start..end].to_owned()),
    }
}

// -- CodeTagFilter ---------------------------------------------------------------

/// Highlight special code tags (such as `TODO`, `FIXME`, `XXX`, `BUG`, `NOTE`)
/// inside comments and docstrings as [`TokenType::COMMENT_SPECIAL`].
#[derive(Debug)]
pub struct CodeTagFilter {
    tag_re: Option<Regex>,
}

impl Default for CodeTagFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeTagFilter {
    /// Filter using Pygments' default tag list (`XXX`, `TODO`, `FIXME`, `BUG`, `NOTE`).
    #[must_use]
    pub fn new() -> Self {
        Self::with_tags(["XXX", "TODO", "FIXME", "BUG", "NOTE"])
    }

    /// Filter using a custom list of tag strings.
    #[must_use]
    pub fn with_tags<I, S>(tags: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut alt = String::new();
        for tag in tags {
            let t = tag.as_ref();
            if t.is_empty() {
                continue;
            }
            if !alt.is_empty() {
                alt.push('|');
            }
            for ch in t.chars() {
                if "\\.^$|?*+()[]{}".contains(ch) {
                    alt.push('\\');
                }
                alt.push(ch);
            }
        }
        let tag_re = (!alt.is_empty()).then(|| {
            let pat = format!(r"\b({alt})\b");
            compile(&pat, "").expect("valid codetag regex")
        });
        Self { tag_re }
    }
}

impl Filter for CodeTagFilter {
    fn filter<'a>(
        &self,
        _lexer: Option<Lexer>,
        stream: Box<dyn Iterator<Item = CowToken<'a>> + 'a>,
    ) -> Result<Vec<CowToken<'a>>, ErrorToken> {
        let mut out = Vec::new();
        for (ty, val) in stream {
            let is_doc_or_comment = ty.is_subtype_of(TokenType::LITERAL_STRING_DOC)
                || (ty.is_subtype_of(TokenType::COMMENT) && !ty.is_subtype_of(TokenType::COMMENT_PREPROC));
            let Some(re) = is_doc_or_comment.then_some(self.tag_re.as_ref()).flatten() else {
                out.push((ty, val));
                continue;
            };
            let mut last = 0;
            for (s, e) in re.find_iter(&val) {
                if s != last {
                    out.push((ty, slice_cow(&val, last, s)));
                }
                out.push((TokenType::COMMENT_SPECIAL, slice_cow(&val, s, e)));
                last = e;
            }
            if last == 0 {
                out.push((ty, val));
            } else if last != val.len() {
                out.push((ty, slice_cow(&val, last, val.len())));
            }
        }
        Ok(out)
    }
}

// -- SymbolFilter ----------------------------------------------------------------

/// Symbol vocabulary accepted by [`SymbolFilter`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SymbolLang {
    /// Isabelle symbols (`\<longrightarrow>` -> `⟶`).
    #[default]
    Isabelle,
    /// LaTeX math symbols (`\longrightarrow` -> `⟶`).
    Latex,
}

/// Convert Isabelle or LaTeX symbol escape tokens into Unicode characters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SymbolFilter {
    lang: SymbolLang,
}

impl SymbolFilter {
    /// Construct a [`SymbolFilter`] for `lang`.
    #[must_use]
    pub const fn new(lang: SymbolLang) -> Self {
        Self { lang }
    }
}

impl Filter for SymbolFilter {
    fn filter<'a>(
        &self,
        _lexer: Option<Lexer>,
        stream: Box<dyn Iterator<Item = CowToken<'a>> + 'a>,
    ) -> Result<Vec<CowToken<'a>>, ErrorToken> {
        let table: &[(&str, &str)] = match self.lang {
            SymbolLang::Isabelle => &ISABELLE_SYMBOLS,
            SymbolLang::Latex => &LATEX_SYMBOLS,
        };
        Ok(stream
            .map(|(ty, val)| {
                if let Ok(idx) = table.binary_search_by_key(&val.as_ref(), |&(k, _)| k) {
                    (ty, Cow::Borrowed(table[idx].1))
                } else {
                    (ty, val)
                }
            })
            .collect())
    }
}

// -- KeywordCaseFilter -----------------------------------------------------------

/// Case transformation applied by [`KeywordCaseFilter`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum KeywordCase {
    /// Convert keywords to lowercase.
    #[default]
    Lower,
    /// Convert keywords to uppercase.
    Upper,
    /// Uppercase the first character and lowercase the rest (Python `str.capitalize`).
    Capitalize,
}

/// Rewrite keyword token text casing (`lower`, `upper`, or `capitalize`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeywordCaseFilter {
    case: KeywordCase,
}

impl KeywordCaseFilter {
    /// Construct a [`KeywordCaseFilter`] using `case`.
    #[must_use]
    pub const fn new(case: KeywordCase) -> Self {
        Self { case }
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    let Some(first) = chars.next() else { return String::new() };
    let mut out = String::with_capacity(s.len());
    for c in first.to_uppercase() {
        out.push(c);
    }
    for c in chars.flat_map(char::to_lowercase) {
        out.push(c);
    }
    out
}

impl Filter for KeywordCaseFilter {
    fn filter<'a>(
        &self,
        _lexer: Option<Lexer>,
        stream: Box<dyn Iterator<Item = CowToken<'a>> + 'a>,
    ) -> Result<Vec<CowToken<'a>>, ErrorToken> {
        Ok(stream
            .map(|(ty, val)| {
                if ty.is_subtype_of(TokenType::KEYWORD) {
                    let converted = match self.case {
                        KeywordCase::Lower => val.to_lowercase(),
                        KeywordCase::Upper => val.to_uppercase(),
                        KeywordCase::Capitalize => capitalize(&val),
                    };
                    (ty, Cow::Owned(converted))
                } else {
                    (ty, val)
                }
            })
            .collect())
    }
}

// -- NameHighlightFilter ---------------------------------------------------------

/// Re-tag selected `Name` tokens (for example function or variable names)
/// with another [`TokenType`] (default [`TokenType::NAME_FUNCTION`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameHighlightFilter {
    names: HashSet<String>,
    token_type: TokenType,
}

impl NameHighlightFilter {
    /// Highlight every token matching `Token.Name` whose text is in `names`.
    #[must_use]
    pub fn new<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self { names: names.into_iter().map(Into::into).collect(), token_type: TokenType::NAME_FUNCTION }
    }

    /// Override the target token type (default [`TokenType::NAME_FUNCTION`]).
    #[must_use]
    pub const fn with_token_type(mut self, ty: TokenType) -> Self {
        self.token_type = ty;
        self
    }
}

impl Filter for NameHighlightFilter {
    fn filter<'a>(
        &self,
        _lexer: Option<Lexer>,
        stream: Box<dyn Iterator<Item = CowToken<'a>> + 'a>,
    ) -> Result<Vec<CowToken<'a>>, ErrorToken> {
        Ok(stream
            .map(|(ty, val)| {
                if ty.is_subtype_of(TokenType::NAME) && self.names.contains(val.as_ref()) {
                    (self.token_type, val)
                } else {
                    (ty, val)
                }
            })
            .collect())
    }
}

// -- RaiseOnErrorTokenFilter -----------------------------------------------------

/// Abort and return [`ErrorToken`] as soon as an [`TokenType::ERROR`] token
/// is seen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RaiseOnErrorTokenFilter;

impl RaiseOnErrorTokenFilter {
    /// Construct a [`RaiseOnErrorTokenFilter`].
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Filter for RaiseOnErrorTokenFilter {
    fn filter<'a>(
        &self,
        _lexer: Option<Lexer>,
        stream: Box<dyn Iterator<Item = CowToken<'a>> + 'a>,
    ) -> Result<Vec<CowToken<'a>>, ErrorToken> {
        let mut out = Vec::new();
        for (ty, val) in stream {
            if ty == TokenType::ERROR {
                return Err(ErrorToken(val.into_owned()));
            }
            out.push((ty, val));
        }
        Ok(out)
    }
}

// -- VisibleWhitespaceFilter -----------------------------------------------------

/// Replace spaces, tabs, and newlines with visible marker characters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisibleWhitespaceFilter {
    spaces: Option<char>,
    tabs: Option<char>,
    tabsize: usize,
    newlines: Option<char>,
    ws_token_type: bool,
}

impl Default for VisibleWhitespaceFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl VisibleWhitespaceFilter {
    /// Construct a [`VisibleWhitespaceFilter`] with no replacements active and
    /// `tabsize = 8`, assigning [`TokenType::TEXT_WHITESPACE`] to whitespace runs.
    #[must_use]
    pub const fn new() -> Self {
        Self { spaces: None, tabs: None, tabsize: 8, newlines: None, ws_token_type: true }
    }

    /// Replace `' '` with Pygments' default middle dot `'·'`.
    #[must_use]
    pub const fn with_default_spaces(self) -> Self {
        self.with_spaces('·')
    }

    /// Replace `' '` with `ch`.
    #[must_use]
    pub const fn with_spaces(mut self, ch: char) -> Self {
        self.spaces = Some(ch);
        self
    }

    /// Replace `'\t'` with Pygments' default `'»'` padded to `tabsize`.
    #[must_use]
    pub const fn with_default_tabs(self) -> Self {
        self.with_tabs('»')
    }

    /// Replace `'\t'` with `ch` padded with spaces to `tabsize`.
    #[must_use]
    pub const fn with_tabs(mut self, ch: char) -> Self {
        self.tabs = Some(ch);
        self
    }

    /// Set the tab width used when tab replacement is enabled (default `8`).
    #[must_use]
    pub const fn with_tabsize(mut self, tabsize: usize) -> Self {
        self.tabsize = tabsize;
        self
    }

    /// Prefix `'\n'` with Pygments' default pilcrow `'¶'`.
    #[must_use]
    pub const fn with_default_newlines(self) -> Self {
        self.with_newlines('¶')
    }

    /// Prefix `'\n'` with `ch`.
    #[must_use]
    pub const fn with_newlines(mut self, ch: char) -> Self {
        self.newlines = Some(ch);
        self
    }

    /// Keep original token types instead of re-tagging whitespace chars as
    /// [`TokenType::TEXT_WHITESPACE`].
    #[must_use]
    pub const fn keep_token_type(mut self) -> Self {
        self.ws_token_type = false;
        self
    }

    fn tab_replacement(&self) -> Option<String> {
        self.tabs.map(|ch| {
            let mut s = String::new();
            s.push(ch);
            for _ in 0..self.tabsize.saturating_sub(1) {
                s.push(' ');
            }
            s
        })
    }
}

impl Filter for VisibleWhitespaceFilter {
    fn filter<'a>(
        &self,
        _lexer: Option<Lexer>,
        stream: Box<dyn Iterator<Item = CowToken<'a>> + 'a>,
    ) -> Result<Vec<CowToken<'a>>, ErrorToken> {
        let tab_str = self.tab_replacement();
        let newline_str = self.newlines.map(|ch| format!("{ch}\n"));
        let mut out = Vec::new();

        if self.ws_token_type {
            for (ty, val) in stream {
                let mut text_start = 0;
                for (i, ch) in val.char_indices() {
                    if !py_whitespace(ch) {
                        continue;
                    }
                    if i != text_start {
                        out.push((ty, slice_cow(&val, text_start, i)));
                    }
                    let repl = match ch {
                        ' ' if self.spaces.is_some() => Cow::Owned(self.spaces.unwrap().to_string()),
                        '\t' if tab_str.is_some() => Cow::Owned(tab_str.clone().unwrap()),
                        '\n' if newline_str.is_some() => Cow::Owned(newline_str.clone().unwrap()),
                        _ => slice_cow(&val, i, i + ch.len_utf8()),
                    };
                    out.push((TokenType::TEXT_WHITESPACE, repl));
                    text_start = i + ch.len_utf8();
                }
                if text_start == 0 {
                    out.push((ty, val));
                } else if text_start != val.len() {
                    out.push((ty, slice_cow(&val, text_start, val.len())));
                }
            }
        } else {
            let space_str = self.spaces.map(|ch| ch.to_string());
            for (ty, val) in stream {
                let needs = (space_str.is_some() && val.contains(' '))
                    || (tab_str.is_some() && val.contains('\t'))
                    || (newline_str.is_some() && val.contains('\n'));
                if !needs {
                    out.push((ty, val));
                    continue;
                }
                let mut replaced = val.into_owned();
                if let Some(ref sp) = space_str {
                    replaced = replaced.replace(' ', sp);
                }
                if let Some(ref tb) = tab_str {
                    replaced = replaced.replace('\t', tb);
                }
                if let Some(ref nl) = newline_str {
                    replaced = replaced.replace('\n', nl);
                }
                out.push((ty, Cow::Owned(replaced)));
            }
        }
        Ok(out)
    }
}

// -- GobbleFilter ----------------------------------------------------------------

/// Strip up to `n` leading characters from every line in the token stream.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GobbleFilter {
    n: usize,
}

impl GobbleFilter {
    /// Construct a [`GobbleFilter`] dropping up to `n` characters per line.
    #[must_use]
    pub const fn new(n: usize) -> Self {
        Self { n }
    }

    fn gobble(s: &str, left: usize) -> (&str, usize) {
        match s.char_indices().nth(left) {
            Some((idx, _)) => (&s[idx..], 0),
            None => ("", left - s.chars().count()),
        }
    }
}

impl Filter for GobbleFilter {
    fn filter<'a>(
        &self,
        _lexer: Option<Lexer>,
        stream: Box<dyn Iterator<Item = CowToken<'a>> + 'a>,
    ) -> Result<Vec<CowToken<'a>>, ErrorToken> {
        let mut left = self.n;
        let mut out = Vec::new();
        for (ty, val) in stream {
            let mut joined = String::new();
            let mut parts = val.split('\n');
            if let Some(first) = parts.next() {
                let (tail, rem) = Self::gobble(first, left);
                left = rem;
                joined.push_str(tail);
            }
            for part in parts {
                joined.push('\n');
                let (tail, rem) = Self::gobble(part, self.n);
                left = rem;
                joined.push_str(tail);
            }
            if !joined.is_empty() {
                out.push((ty, Cow::Owned(joined)));
            }
        }
        Ok(out)
    }
}

// -- TokenMergeFilter ------------------------------------------------------------

/// Merge consecutive tokens with the same [`TokenType`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TokenMergeFilter;

impl TokenMergeFilter {
    /// Construct a [`TokenMergeFilter`].
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Filter for TokenMergeFilter {
    fn filter<'a>(
        &self,
        _lexer: Option<Lexer>,
        stream: Box<dyn Iterator<Item = CowToken<'a>> + 'a>,
    ) -> Result<Vec<CowToken<'a>>, ErrorToken> {
        let mut out: Vec<CowToken<'a>> = Vec::new();
        for (ty, val) in stream {
            match out.last_mut() {
                Some((cur_ty, cur_val)) if *cur_ty == ty => cur_val.to_mut().push_str(&val),
                _ => out.push((ty, val)),
            }
        }
        Ok(out)
    }
}

// -- Registry lookup -------------------------------------------------------------

static FILTER_NAMES: [&str; 8] =
    ["codetagify", "gobble", "highlight", "keywordcase", "raiseonerror", "symbols", "tokenmerge", "whitespace"];

/// Names of all built-in Pygments filters, sorted.
#[must_use]
pub fn filter_names() -> &'static [&'static str] {
    &FILTER_NAMES
}

fn parse_bool(s: &str) -> Result<bool, FilterError> {
    match s.to_ascii_lowercase().as_str() {
        "1" | "yes" | "true" | "on" => Ok(true),
        "0" | "no" | "false" | "off" => Ok(false),
        _ => Err(FilterError(format!("invalid boolean option value {s:?}"))),
    }
}

fn parse_ws_opt(val: &str, default: char) -> Result<Option<char>, FilterError> {
    if val.is_empty() {
        return Ok(None);
    }
    let mut chars = val.chars();
    if let (Some(ch), None) = (chars.next(), chars.next()) {
        return Ok(Some(ch));
    }
    if parse_bool(val)? { Ok(Some(default)) } else { Ok(None) }
}

/// Look up and instantiate a filter by Pygments name (`"codetagify"`,
/// `"keywordcase"`, `"highlight"`, `"raiseonerror"`, `"whitespace"`,
/// `"gobble"`, `"tokenmerge"`, `"symbols"`) with string key-value options.
///
/// # Errors
///
/// Returns [`FilterError`] if `name` is unknown or any option value is invalid.
pub fn filter_by_name(name: &str, options: &[(&str, &str)]) -> Result<Box<dyn Filter>, FilterError> {
    let opt = |key: &str| options.iter().rfind(|&&(k, _)| k == key).map(|&(_, v)| v);
    match name {
        "codetagify" => Ok(match opt("codetags") {
            Some(s) => Box::new(CodeTagFilter::with_tags(s.split_whitespace())),
            None => Box::new(CodeTagFilter::new()),
        }),
        "symbols" => {
            let lang = match opt("lang") {
                None | Some("isabelle") => SymbolLang::Isabelle,
                Some("latex") => SymbolLang::Latex,
                Some(other) => return Err(FilterError(format!("unknown symbols lang {other:?}"))),
            };
            Ok(Box::new(SymbolFilter::new(lang)))
        }
        "keywordcase" => {
            let case = match opt("case") {
                None | Some("lower") => KeywordCase::Lower,
                Some("upper") => KeywordCase::Upper,
                Some("capitalize") => KeywordCase::Capitalize,
                Some(other) => return Err(FilterError(format!("unknown keywordcase {other:?}"))),
            };
            Ok(Box::new(KeywordCaseFilter::new(case)))
        }
        "highlight" => {
            let names = opt("names").unwrap_or("").split_whitespace();
            let mut f = NameHighlightFilter::new(names);
            if let Some(tt) = opt("tokentype") {
                let ty = TokenType::from_name(tt).ok_or_else(|| FilterError(format!("unknown token type {tt:?}")))?;
                f = f.with_token_type(ty);
            }
            Ok(Box::new(f))
        }
        "raiseonerror" => Ok(Box::new(RaiseOnErrorTokenFilter::new())),
        "whitespace" => {
            let mut f = VisibleWhitespaceFilter::new();
            if let Some(v) = opt("spaces") {
                f.spaces = parse_ws_opt(v, '·')?;
            }
            if let Some(v) = opt("tabs") {
                f.tabs = parse_ws_opt(v, '»')?;
            }
            if let Some(v) = opt("tabsize") {
                f.tabsize = v.parse().map_err(|_| FilterError(format!("invalid tabsize {v:?}")))?;
            }
            if let Some(v) = opt("newlines") {
                f.newlines = parse_ws_opt(v, '¶')?;
            }
            if let Some(v) = opt("wstokentype")
                && !parse_bool(v)?
            {
                f = f.keep_token_type();
            }
            Ok(Box::new(f))
        }
        "gobble" => {
            let n = match opt("n") {
                Some(v) => v.parse().map_err(|_| FilterError(format!("invalid gobble n {v:?}")))?,
                None => 0,
            };
            Ok(Box::new(GobbleFilter::new(n)))
        }
        "tokenmerge" => Ok(Box::new(TokenMergeFilter::new())),
        _ => Err(FilterError(format!("filter {name:?} not found"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codetag_and_raise_and_merge() {
        let ct = CodeTagFilter::new();
        let toks = [(TokenType::COMMENT_SINGLE, "// TODO fix"), (TokenType::COMMENT_SINGLE, " later")];
        let after = apply_filters(None, toks, &[&ct]).unwrap();
        assert_eq!(
            after,
            vec![
                (TokenType::COMMENT_SINGLE, Cow::Borrowed("// ")),
                (TokenType::COMMENT_SPECIAL, Cow::Borrowed("TODO")),
                (TokenType::COMMENT_SINGLE, Cow::Borrowed(" fix")),
                (TokenType::COMMENT_SINGLE, Cow::Borrowed(" later")),
            ]
        );
        let merged = TokenMergeFilter::new()
            .filter(
                None,
                Box::new([(TokenType::TEXT, Cow::Borrowed("a")), (TokenType::TEXT, Cow::Borrowed("b"))].into_iter()),
            )
            .unwrap();
        assert_eq!(merged, vec![(TokenType::TEXT, Cow::Borrowed("ab"))]);

        let err = apply_filters(None, [(TokenType::ERROR, "bad")], &[&RaiseOnErrorTokenFilter::new()]);
        assert_eq!(err, Err(ErrorToken("bad".into())));
    }

    #[test]
    fn symbols_keywordcase_name_gobble_whitespace() {
        let sym_iso = SymbolFilter::new(SymbolLang::Isabelle);
        let sym_tex = SymbolFilter::new(SymbolLang::Latex);
        assert_eq!(
            apply_filters(None, [(TokenType::OPERATOR, r"\<longrightarrow>")], &[&sym_iso]).unwrap()[0].1,
            "\u{27f6}"
        );
        assert_eq!(apply_filters(None, [(TokenType::NAME_BUILTIN, r"\alpha")], &[&sym_tex]).unwrap()[0].1, "\u{03b1}");

        let kw = KeywordCaseFilter::new(KeywordCase::Capitalize);
        assert_eq!(apply_filters(None, [(TokenType::KEYWORD, "bEgIn")], &[&kw]).unwrap()[0].1, "Begin");

        let hl = NameHighlightFilter::new(["foo"]);
        assert_eq!(
            apply_filters(None, [(TokenType::NAME, "foo"), (TokenType::NAME, "bar")], &[&hl]).unwrap(),
            vec![(TokenType::NAME_FUNCTION, Cow::Borrowed("foo")), (TokenType::NAME, Cow::Borrowed("bar")),]
        );

        let gob = GobbleFilter::new(2);
        let g = apply_filters(None, [(TokenType::TEXT, "  ab\n    cd\n"), (TokenType::TEXT, " x")], &[&gob]).unwrap();
        assert_eq!(
            g,
            vec![(TokenType::TEXT, Cow::Borrowed("ab\n  cd\n")), (TokenType::TEXT, Cow::Borrowed(""))]
                .into_iter()
                .filter(|(_, s)| !s.is_empty())
                .collect::<Vec<_>>()
        );

        let ws = VisibleWhitespaceFilter::new().with_default_spaces().with_default_tabs().with_tabsize(4);
        let w = apply_filters(None, [(TokenType::TEXT, "a \tb")], &[&ws]).unwrap();
        assert_eq!(
            w,
            vec![
                (TokenType::TEXT, Cow::Borrowed("a")),
                (TokenType::TEXT_WHITESPACE, Cow::Borrowed("·")),
                (TokenType::TEXT_WHITESPACE, Cow::Borrowed("»   ")),
                (TokenType::TEXT, Cow::Borrowed("b")),
            ]
        );
    }

    #[test]
    fn registry_lookup() {
        assert_eq!(filter_names().len(), 8);
        for &name in filter_names() {
            assert!(filter_by_name(name, &[]).is_ok(), "{name}");
        }
        assert!(filter_by_name("missing", &[]).is_err());
    }
}
