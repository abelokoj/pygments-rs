//! Lexers: loading the bundled data, the public [`Lexer`] handle, the
//! [`Tokens`] iterator and resumable [`LexState`].

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::ops::Range;
use std::sync::LazyLock;

use serde::Deserialize;
use serde_json::Value;

use crate::engine::{RegexLexer, RuleData, Run, StateId, Tok};
use crate::hooks::{self, CustomFn, Extra};
use crate::registry::{self, LexerInfo};
use crate::token::TokenType;

#[derive(Deserialize)]
struct LexerData {
    kind: String,
    #[serde(default)]
    extended: bool,
    #[serde(default)]
    regexes: Vec<Vec<String>>,
    #[serde(default)]
    states: serde_json::Map<String, Value>,
    #[serde(default)]
    initial_stack: Vec<String>,
    #[serde(default)]
    root: Option<String>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    needle: Option<String>,
    #[serde(default)]
    consts: HashMap<String, Value>,
}

pub(crate) enum Kind {
    Regex(RegexLexer),
    Delegating {
        root: Box<str>,
        language: Box<str>,
        needle: TokenType,
    },
    Custom(CustomFn),
    /// A hand-written Pygments lexer without a Rust port yet; lexes as plain text.
    Unported,
}

/// A constant a hook needs (word lists, flags), as exported by the converter.
pub(crate) struct Const {
    pub value: Value,
    pub set: HashSet<Box<str>>,
}

pub(crate) struct LexerImpl {
    pub(crate) info: &'static LexerInfo,
    pub(crate) kind: Kind,
    consts: HashMap<Box<str>, Const>,
}

impl LexerImpl {
    /// Build from bundled data. Panics only if the bundled data is corrupt,
    /// which the test suite rules out for every lexer.
    pub(crate) fn build(info: &'static LexerInfo) -> LexerImpl {
        let data: LexerData =
            serde_json::from_str(info.data).unwrap_or_else(|e| panic!("{}: bad bundled data: {e}", info.class));
        let kind = match data.kind.as_str() {
            "regex" => {
                let states = data
                    .states
                    .into_iter()
                    .map(|(name, rules)| {
                        let rules: Vec<RuleData> = serde_json::from_value(rules)
                            .unwrap_or_else(|e| panic!("{}: bad state {name}: {e}", info.class));
                        (name, rules)
                    })
                    .collect();
                Kind::Regex(RegexLexer::build(info.class, data.extended, data.regexes, states, &data.initial_stack))
            }
            "delegating" => Kind::Delegating {
                root: data.root.unwrap_or_default().into(),
                language: data.language.unwrap_or_default().into(),
                needle: data.needle.as_deref().and_then(TokenType::from_name).unwrap_or(TokenType::OTHER),
            },
            _ => hooks::custom_for(info.class).map_or(Kind::Unported, Kind::Custom),
        };
        let consts = data
            .consts
            .into_iter()
            .map(|(k, value)| {
                let set = match &value {
                    Value::Array(items) => items.iter().filter_map(Value::as_str).map(Box::from).collect(),
                    _ => HashSet::new(),
                };
                (k.into_boxed_str(), Const { value, set })
            })
            .collect();
        LexerImpl { info, kind, consts }
    }

    pub(crate) fn regex(&self) -> Option<&RegexLexer> {
        match &self.kind {
            Kind::Regex(r) => Some(r),
            _ => None,
        }
    }

    /// A constant exported for this lexer's hooks.
    pub(crate) fn konst(&self, name: &str) -> Option<&Value> {
        self.consts.get(name).map(|c| &c.value)
    }

    /// A string-list constant as a set; empty if absent.
    pub(crate) fn word_set(&self, name: &str) -> &HashSet<Box<str>> {
        static EMPTY: LazyLock<HashSet<Box<str>>> = LazyLock::new(HashSet::new);
        self.consts.get(name).map_or(&EMPTY, |c| &c.set)
    }

    /// Pygments' `get_tokens_unprocessed(text, stack)`: tokens for all of
    /// `text`, appended to `out` with offsets into `text`.
    pub(crate) fn tokens_unprocessed(&'static self, text: &str, stack: Option<&[&str]>, out: &mut Vec<Tok>) {
        match &self.kind {
            Kind::Regex(rl) => {
                let stack = stack.and_then(|names| rl.stack_from_names(names)).unwrap_or_default();
                Run::new(self, text, 0, stack, Extra::default()).run_into(out);
            }
            Kind::Delegating { root, language, needle } => {
                match (registry::by_class(root), registry::by_class(language)) {
                    (Some(r), Some(l)) => hooks::delegate(r, l, *needle, text, out),
                    _ => out.push(Tok::new(TokenType::TEXT, 0, text.len())),
                }
            }
            Kind::Custom(f) => f(self, text, out),
            Kind::Unported => {
                if !text.is_empty() {
                    out.push(Tok::new(TokenType::TEXT, 0, text.len()));
                }
            }
        }
    }
}

/// A lexer for one language. Cheap to copy; the rule tables are loaded and
/// compiled lazily on first use and then shared process-wide.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Lexer(pub(crate) u16);

impl Lexer {
    fn info(self) -> &'static LexerInfo {
        registry::info(self.0)
    }

    pub(crate) fn imp(self) -> &'static LexerImpl {
        registry::get(self.0)
    }

    /// Human-readable name, e.g. `"Python"`.
    pub fn name(self) -> &'static str {
        self.info().name
    }

    /// Pygments class name, e.g. `"PythonLexer"`; unique per lexer.
    pub fn class_name(self) -> &'static str {
        self.info().class
    }

    /// Short names accepted by [`crate::lexer_by_name`], e.g. `["python", "py"]`.
    pub fn aliases(self) -> &'static [&'static str] {
        self.info().aliases
    }

    /// File name globs, e.g. `["*.py", "*.pyw"]`.
    pub fn filenames(self) -> &'static [&'static str] {
        self.info().filenames
    }

    /// MIME types, e.g. `["text/x-python"]`.
    pub fn mimetypes(self) -> &'static [&'static str] {
        self.info().mimetypes
    }

    /// True if this lexer is driven by a state machine whose state can be
    /// saved at line starts and resumed (see [`Lexer::tokenize_from`]).
    /// Delegating and hand-written lexers always re-lex from the start.
    pub fn is_resumable(self) -> bool {
        matches!(self.imp().kind, Kind::Regex(_))
    }

    /// True if Pygments implements this lexer in Python code that has not
    /// been ported yet; such a lexer returns the whole text as `Token.Text`.
    pub fn is_unported(self) -> bool {
        matches!(self.imp().kind, Kind::Unported)
    }

    /// How likely `text` is in this language, in `[0, 1]` (Pygments'
    /// `analyse_text`).
    pub fn analyse_text(self, text: &str) -> f32 {
        crate::analyse::score(self.info().class, text)
    }

    /// Normalize input the way Pygments' `get_tokens` does before lexing:
    /// drop a BOM, turn `\r\n` and `\r` into `\n`, strip leading and
    /// trailing newlines, expand tabs if the lexer asks for it and make sure
    /// the text ends with `\n`. Lexing the result reproduces Pygments' output
    /// exactly; editors that need offsets into their own buffer should lex the
    /// raw text instead.
    pub fn preprocess(self, text: &str) -> Cow<'_, str> {
        let info = self.info();
        let mut t: Cow<'_, str> = Cow::Borrowed(text.strip_prefix('\u{feff}').unwrap_or(text));
        if t.contains('\r') {
            t = Cow::Owned(t.replace("\r\n", "\n").replace('\r', "\n"));
        }
        let trimmed: &str = if info.stripall {
            t.trim_matches(py_whitespace)
        } else if info.stripnl {
            t.trim_matches('\n')
        } else {
            &t
        };
        if trimmed.len() != t.len() {
            t = Cow::Owned(trimmed.to_owned());
        }
        if info.tabsize > 0 && t.contains('\t') {
            t = Cow::Owned(expand_tabs(&t, info.tabsize));
        }
        if info.ensurenl && !t.ends_with('\n') {
            t.to_mut().push('\n');
        }
        t
    }

    /// Tokenize `text` from the start. Concatenating the token texts gives
    /// back `text` (for every lexer Pygments' own tests check).
    pub fn tokenize(self, text: &str) -> Tokens<'_> {
        self.tokenize_from(text, 0, &self.initial_state())
    }

    /// The state at the start of a text.
    pub fn initial_state(self) -> LexState {
        LexState { stack: Vec::new(), extra: Extra::default() }
    }

    /// Resume tokenizing `text` at byte `offset` from a state saved with
    /// [`Tokens::state`]. Regexes still see the text before `offset`, so
    /// look-behind and `^` behave exactly as in a full run.
    ///
    /// For lexers that are not [resumable](Lexer::is_resumable) the state is
    /// ignored and the whole text is lexed again; tokens before `offset` are
    /// skipped.
    pub fn tokenize_from<'t>(self, text: &'t str, offset: usize, state: &LexState) -> Tokens<'t> {
        let imp = self.imp();
        let offset = offset.min(text.len());
        let inner = match &imp.kind {
            Kind::Regex(rl) => {
                let valid = state.stack.iter().all(|&s| rl.has_state(s));
                let (stack, extra) =
                    if valid { (state.stack.clone(), state.extra.clone()) } else { (Vec::new(), Extra::default()) };
                Inner::Regex(Box::new(Run::new(imp, text, offset, stack, extra)))
            }
            _ => {
                let mut toks = Vec::new();
                imp.tokens_unprocessed(text, None, &mut toks);
                toks.retain(|t| t.start >= offset);
                Inner::Buffered(toks.into_iter())
            }
        };
        Tokens { text, inner }
    }

    /// Every pattern of this lexer that Oniguruma rejects, with the error.
    /// Used by the test suite; such rules never match.
    pub fn regex_errors(self) -> Vec<(String, String)> {
        self.imp().regex().map(RegexLexer::regex_errors).unwrap_or_default()
    }
}

impl fmt::Debug for Lexer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Lexer({})", self.class_name())
    }
}

fn py_whitespace(c: char) -> bool {
    // Python's str.strip() default set: Unicode whitespace plus the ASCII
    // separators 0x1c-0x1f that Rust does not count as whitespace.
    c.is_whitespace() || ('\x1c'..='\x1f').contains(&c)
}

/// Python's `str.expandtabs(tabsize)`.
fn expand_tabs(s: &str, tabsize: usize) -> String {
    let mut out = String::with_capacity(s.len());
    let mut col = 0;
    for c in s.chars() {
        match c {
            '\t' => {
                let n = tabsize - col % tabsize;
                out.extend(std::iter::repeat_n(' ', n));
                col += n;
            }
            '\n' | '\r' => {
                out.push(c);
                col = 0;
            }
            _ => {
                out.push(c);
                col += 1;
            }
        }
    }
    out
}

/// Lexer state at a token boundary: the state stack plus any state a ported
/// callback keeps. Save it at line starts and pass it to
/// [`Lexer::tokenize_from`] to re-lex only from an edited line; stop once the
/// state at a later line start equals the saved one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LexState {
    stack: Vec<StateId>,
    extra: Extra,
}

/// Iterator over `(token type, text)` pairs. Created by [`Lexer::tokenize`].
pub struct Tokens<'t> {
    text: &'t str,
    inner: Inner<'t>,
}

enum Inner<'t> {
    Regex(Box<Run<'t>>),
    Buffered(std::vec::IntoIter<Tok>),
}

impl<'t> Tokens<'t> {
    /// The next token as a type and a byte range into the text.
    pub fn next_span(&mut self) -> Option<(TokenType, Range<usize>)> {
        let tok = match &mut self.inner {
            Inner::Regex(run) => loop {
                if let Some(t) = run.out.pop_front() {
                    break Some(t);
                }
                if !run.step() {
                    break run.out.pop_front();
                }
            },
            Inner::Buffered(it) => it.next(),
        }?;
        Some((tok.ty, tok.start..tok.end))
    }

    /// Byte offset where the next token starts (the text length at the end).
    pub fn offset(&self) -> usize {
        match &self.inner {
            Inner::Regex(run) => run.out.front().map_or(run.pos, |t| t.start),
            Inner::Buffered(it) => it.as_slice().first().map_or(self.text.len(), |t| t.start),
        }
    }

    /// The state to resume from at [`Tokens::offset`], or `None` if the
    /// lexer is in the middle of a multi-token match there or is not
    /// resumable.
    pub fn state(&self) -> Option<LexState> {
        match &self.inner {
            Inner::Regex(run) if run.out.is_empty() => {
                Some(LexState { stack: run.stack.clone(), extra: run.extra.clone() })
            }
            _ => None,
        }
    }
}

impl<'t> Iterator for Tokens<'t> {
    type Item = (TokenType, &'t str);

    fn next(&mut self) -> Option<Self::Item> {
        let (ty, r) = self.next_span()?;
        // Hand-ported lexers compute offsets; never panic on a bad one.
        Some((ty, self.text.get(r).unwrap_or("")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_tabs_matches_python() {
        assert_eq!(expand_tabs("a\tb", 4), "a   b");
        assert_eq!(expand_tabs("\tx\n\ty", 2), "  x\n  y");
    }
}
