//! Hand-ported Python pieces that the converter cannot express as data:
//! rule callbacks, `get_tokens_unprocessed` overrides (token filters), fully
//! custom lexers and `analyse_text` heuristics.
//!
//! Each submodule ports one Pygments module and exposes up to four tables;
//! the lookups below search them. Keys match `data/report.json`:
//! callbacks by `"<module>::<qualname>"`, everything else by class name.

use std::any::Any;
use std::collections::VecDeque;
use std::fmt;

use onig::Region;
use serde_json::Value;

use crate::engine::{StateId, Tok};
use crate::lexer::LexerImpl;
use crate::token::TokenType;

/// A rule action implemented in Rust (a Python callback in Pygments).
pub(crate) type CallbackFn = fn(&mut Cx<'_>, &Groups<'_>, &Value);
/// A `get_tokens_unprocessed` override that rewrites the token stream.
/// Receives each token the lexer produces and pushes zero or more to `out`.
pub(crate) type FilterFn = fn(&'static LexerImpl, &str, Tok, &mut Extra, &mut VecDeque<Tok>);
/// A lexer written by hand: append tokens for all of `text` to `out`.
pub(crate) type CustomFn = fn(&'static LexerImpl, &str, &mut Vec<Tok>);
/// An `analyse_text` heuristic returning a score in `[0, 1]`.
pub(crate) type AnalyseFn = fn(&str) -> f32;

/// The tables one ported module contributes.
pub(crate) struct Module {
    pub callbacks: &'static [(&'static str, CallbackFn)],
    pub filters: &'static [(&'static str, FilterFn)],
    pub custom: &'static [(&'static str, CustomFn)],
    pub analyse: &'static [(&'static str, AnalyseFn)],
}

impl Module {
    pub(crate) const EMPTY: Module = Module { callbacks: &[], filters: &[], custom: &[], analyse: &[] };
}

mod analysers;
mod callbacks_a;
mod callbacks_b;
mod consoles;
mod custom;
mod filters;

/// Every ported module. Add new ones here.
static MODULES: &[&Module] = &[
    &analysers::MODULE,
    &callbacks_a::MODULE,
    &callbacks_b::MODULE,
    &consoles::MODULE,
    &custom::MODULE,
    &filters::MODULE,
];

fn find<T: Copy>(key: &str, table: fn(&'static Module) -> &'static [(&'static str, T)]) -> Option<T> {
    MODULES.iter().flat_map(|m| table(*m).iter()).find(|(k, _)| *k == key).map(|(_, v)| *v)
}

pub(crate) fn callback_for(key: &str) -> Option<CallbackFn> {
    find(key, |m| m.callbacks)
}

pub(crate) fn filter_for(class: &str) -> Option<FilterFn> {
    find(class, |m| m.filters)
}

pub(crate) fn custom_for(class: &str) -> Option<CustomFn> {
    find(class, |m| m.custom)
}

pub(crate) fn analyse_for(class: &str) -> Option<AnalyseFn> {
    find(class, |m| m.analyse)
}

/// Every key that has a Rust port, for the coverage report.
pub(crate) fn ported_keys() -> impl Iterator<Item = &'static str> {
    MODULES.iter().flat_map(|m| {
        m.callbacks
            .iter()
            .map(|e| e.0)
            .chain(m.filters.iter().map(|e| e.0))
            .chain(m.custom.iter().map(|e| e.0))
            .chain(m.analyse.iter().map(|e| e.0))
    })
}

// -- callback context -----------------------------------------------------------

/// Capture groups of the current match (or a group-less pseudo match, which is
/// what Pygments hands to callbacks nested in `bygroups`).
pub(crate) struct Groups<'a> {
    region: Option<&'a Region>,
    start: usize,
    end: usize,
}

impl<'a> Groups<'a> {
    pub(crate) fn from_region(region: &'a Region, start: usize, end: usize) -> Groups<'a> {
        Groups { region: Some(region), start, end }
    }

    pub(crate) fn pseudo(start: usize, end: usize) -> Groups<'static> {
        Groups { region: None, start, end }
    }

    /// Byte range of group `i` (0 is the whole match); `None` if it did not participate.
    pub(crate) fn get(&self, i: usize) -> Option<(usize, usize)> {
        if i == 0 { Some((self.start, self.end)) } else { self.region?.pos(i) }
    }

    pub(crate) fn start(&self) -> usize {
        self.start
    }

    pub(crate) fn end(&self) -> usize {
        self.end
    }

    /// Text of group `i`, like Python's `match.group(i)`.
    pub(crate) fn str<'t>(&self, text: &'t str, i: usize) -> Option<&'t str> {
        self.get(i).and_then(|(s, e)| text.get(s..e))
    }
}

/// What a callback can see and change: Pygments' `(lexer, match, ctx)`.
pub(crate) struct Cx<'a> {
    pub lexer: &'static LexerImpl,
    /// The full text being lexed (offsets in `Groups` index into it).
    pub text: &'a str,
    /// `ctx.pos` for `ExtendedRegexLexer` callbacks; the engine ignores it otherwise.
    pub pos: usize,
    pub stack: &'a mut Vec<StateId>,
    /// Per-tokenization state (Pygments stores it on `ctx` or the lexer instance).
    pub extra: &'a mut Extra,
    pub out: &'a mut Vec<Tok>,
}

impl Cx<'_> {
    pub(crate) fn emit(&mut self, ty: TokenType, start: usize, end: usize) {
        self.out.push(Tok::new(ty, start, end));
    }

    /// State id by name; panics on a name the lexer does not define (a port bug).
    pub(crate) fn state(&self, name: &str) -> StateId {
        self.lexer
            .regex()
            .and_then(|r| r.state_id(name))
            .unwrap_or_else(|| panic!("{}: no state {name:?}", self.lexer.info.class))
    }

    pub(crate) fn push_state(&mut self, name: &str) {
        let id = self.state(name);
        self.stack.push(id);
    }

    /// `#pop` semantics: never pops the last state.
    pub(crate) fn pop_state(&mut self) {
        if self.stack.len() > 1 {
            self.stack.pop();
        }
    }

    /// Lex `text[start..end]` with the lexer named by `class` (Pygments class
    /// name) as a separate text, like `using(OtherLexer)`.
    pub(crate) fn lex_with(&mut self, class: &str, start: usize, end: usize, stack: Option<&[&str]>) {
        match crate::registry::by_class(class) {
            Some(l) => lex_sub(l, self.text, start, end, stack, self.out),
            None => self.emit(TokenType::TOKEN, start, end),
        }
    }

    /// Like `lex_with`, but with this same lexer (`using(this)`).
    pub(crate) fn lex_this(&mut self, start: usize, end: usize, stack: Option<&[&str]>) {
        lex_sub(self.lexer, self.text, start, end, stack, self.out)
    }
}

/// Lex `text[start..end]` as its own text and shift the offsets back.
pub(crate) fn lex_sub(
    lexer: &'static LexerImpl,
    text: &str,
    start: usize,
    end: usize,
    stack: Option<&[&str]>,
    out: &mut Vec<Tok>,
) {
    let Some(sub) = text.get(start..end) else { return };
    let first = out.len();
    lexer.tokens_unprocessed(sub, stack, out);
    for t in &mut out[first..] {
        t.start += start;
        t.end += start;
    }
}

// -- per-tokenization extra state ------------------------------------------------

/// Object-safe view of a callback's private state type.
pub(crate) trait ExtraState: Any + Send + Sync + fmt::Debug {
    fn clone_box(&self) -> Box<dyn ExtraState>;
    fn eq_dyn(&self, other: &dyn ExtraState) -> bool;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<T: Any + Clone + PartialEq + Send + Sync + fmt::Debug> ExtraState for T {
    fn clone_box(&self) -> Box<dyn ExtraState> {
        Box::new(self.clone())
    }

    fn eq_dyn(&self, other: &dyn ExtraState) -> bool {
        other.as_any().downcast_ref::<T>() == Some(self)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// State a ported callback keeps between matches (YAML indentation, Ruby
/// heredoc queue, ...). It is part of [`crate::LexState`], so lexers that use
/// it still resume correctly.
#[derive(Default, Debug)]
pub(crate) struct Extra(Option<Box<dyn ExtraState>>);

impl Extra {
    /// The state of type `T`, created with `T::default()` on first use.
    pub(crate) fn get<T: ExtraState + Default>(&mut self) -> &mut T {
        let fresh = !matches!(&self.0, Some(b) if (**b).as_any().is::<T>());
        if fresh {
            self.0 = Some(Box::new(T::default()));
        }
        self.0.as_mut().and_then(|b| (**b).as_any_mut().downcast_mut::<T>()).expect("just inserted")
    }
}

impl Clone for Extra {
    fn clone(&self) -> Extra {
        Extra(self.0.as_ref().map(|b| (**b).clone_box()))
    }
}

impl PartialEq for Extra {
    fn eq(&self, other: &Extra) -> bool {
        match (&self.0, &other.0) {
            (None, None) => true,
            (Some(a), Some(b)) => (**a).eq_dyn(&**b),
            _ => false,
        }
    }
}

impl Eq for Extra {}

// -- shared helpers for ports ------------------------------------------------------

/// Port of `pygments.lexer.do_insertions`: splice the `insertions` (each a
/// position in the code buffer and the tokens to insert there) into the code
/// `tokens`. Positions of the result are cumulative from the first token, as
/// in Python, so they index the original text when the pieces cover it.
pub(crate) fn do_insertions(insertions: Vec<(usize, Vec<Tok>)>, tokens: Vec<Tok>) -> Vec<Tok> {
    fn put(out: &mut Vec<Tok>, rp: &mut usize, ty: TokenType, len: usize) {
        out.push(Tok::new(ty, *rp, *rp + len));
        *rp += len;
    }
    let mut ins = insertions.into_iter();
    let Some((mut index, mut itokens)) = ins.next() else { return tokens };
    let mut out = Vec::with_capacity(tokens.len());
    let mut realpos: Option<usize> = None;
    let mut insleft = true;
    for t in tokens {
        let i = t.start;
        let vlen = t.end - t.start;
        let rp = realpos.get_or_insert(i);
        let mut oldi = 0;
        while insleft && i + vlen >= index {
            let cut = index.saturating_sub(i).min(vlen);
            if cut > oldi {
                put(&mut out, rp, t.ty, cut - oldi);
            }
            for it in itokens.drain(..) {
                put(&mut out, rp, it.ty, it.end - it.start);
            }
            oldi = cut;
            match ins.next() {
                Some((a, b)) => (index, itokens) = (a, b),
                None => insleft = false,
            }
        }
        if oldi < vlen {
            put(&mut out, rp, t.ty, vlen - oldi);
        }
    }
    if insleft {
        // Insertions past the last code token.
        let rp = realpos.get_or_insert(0);
        for it in itokens.into_iter().chain(ins.flat_map(|(_, toks)| toks)) {
            put(&mut out, rp, it.ty, it.end - it.start);
        }
    }
    out
}

/// Port of `DelegatingLexer.get_tokens_unprocessed`.
pub(crate) fn delegate(
    root: &'static LexerImpl,
    language: &'static LexerImpl,
    needle: TokenType,
    text: &str,
    out: &mut Vec<Tok>,
) {
    let mut lang = Vec::new();
    language.tokens_unprocessed(text, None, &mut lang);
    let mut buffered = String::new();
    let mut insertions = Vec::new();
    let mut lng_buffer = Vec::new();
    for t in lang {
        if t.ty == needle {
            if !lng_buffer.is_empty() {
                insertions.push((buffered.len(), std::mem::take(&mut lng_buffer)));
            }
            buffered.push_str(text.get(t.start..t.end).unwrap_or(""));
        } else {
            lng_buffer.push(t);
        }
    }
    if !lng_buffer.is_empty() {
        insertions.push((buffered.len(), lng_buffer));
    }
    let mut root_toks = Vec::new();
    root.tokens_unprocessed(&buffered, None, &mut root_toks);
    out.extend(do_insertions(insertions, root_toks));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(ty: TokenType, s: usize, e: usize) -> Tok {
        Tok::new(ty, s, e)
    }

    #[test]
    fn do_insertions_splits_tokens() {
        // Code "abcdef" lexed as one Text token; insert a prompt at 3 and 6.
        let code = vec![t(TokenType::TEXT, 0, 6)];
        let ins = vec![(0, vec![t(TokenType::GENERIC_PROMPT, 0, 2)]), (3, vec![t(TokenType::GENERIC_PROMPT, 0, 2)])];
        let got = do_insertions(ins, code);
        assert_eq!(
            got,
            vec![
                t(TokenType::GENERIC_PROMPT, 0, 2),
                t(TokenType::TEXT, 2, 5),
                t(TokenType::GENERIC_PROMPT, 5, 7),
                t(TokenType::TEXT, 7, 10),
            ]
        );
    }

    #[test]
    fn do_insertions_leftover() {
        let got = do_insertions(vec![(5, vec![t(TokenType::GENERIC_OUTPUT, 0, 4)])], vec![]);
        assert_eq!(got, vec![t(TokenType::GENERIC_OUTPUT, 0, 4)]);
    }

    #[derive(Clone, Debug, Default, PartialEq)]
    struct Counter(u32);

    #[test]
    fn extra_state_roundtrip() {
        let mut e = Extra::default();
        e.get::<Counter>().0 += 2;
        let c = e.clone();
        assert_eq!(e, c);
        e.get::<Counter>().0 += 1;
        assert_ne!(e, c);
        assert_eq!(Extra::default(), Extra::default());
    }
}
