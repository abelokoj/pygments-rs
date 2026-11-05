//! `TNTLexer` from `pygments.lexers.tnt` (Typographic Number Theory): a
//! recursive-descent tokenizer with Python's exception-driven recovery.

use std::sync::LazyLock;

use onig::{Regex, Region, SearchOptions};

use super::match_at;
use crate::engine::{Tok, compile};
use crate::lexer::LexerImpl;
use crate::token::TokenType;

static RULES: LazyLock<Regex> = LazyLock::new(|| {
    compile(
        r"(?xi)
        joining | separation | double-tilde | fantasy\ rule
        | carry[- ]over(?:\ of)?(?:\ line)?\ ([0-9]+) | detachment
        | contrapositive | De\ Morgan | switcheroo
        | specification | generalization | interchange
        | existence | symmetry | transitivity
        | add\ S | drop\ S | induction
        | axiom\ ([1-5]) | premise | push | pop
    ",
        "ix",
    )
    .expect("static TNT pattern")
});
static LINENOS: LazyLock<Regex> =
    LazyLock::new(|| compile(r"(?:[0-9]+)(?:(?:, ?|,? and )(?:[0-9]+))*", "").expect("static TNT pattern"));
static COMMENT: LazyLock<Regex> = LazyLock::new(|| compile(r"\[[^\n\]]+\]", "").expect("static TNT pattern"));

/// Python's `AssertionError` (or `RecursionError`) and `IndexError`.
#[derive(Clone, Copy, PartialEq, Debug)]
enum E {
    Assert,
    Index,
}

type R = Result<usize, E>;

/// Below CPython's default recursion limit; deeper nesting fails like
/// `RecursionError` does.
const MAX_DEPTH: u32 = 950;

fn is_logic(c: char) -> bool {
    matches!(c, '⊃' | '→' | ']' | '&' | '∧' | '^' | '|' | '∨' | 'V' | 'v')
}
fn is_operator(c: char) -> bool {
    matches!(c, '+' | '.' | '⋅' | '*')
}
fn is_variable(c: char) -> bool {
    matches!(c, 'a'..='e')
}
fn is_prime(c: char) -> bool {
    matches!(c, '\'' | '′')
}
fn is_negator(c: char) -> bool {
    matches!(c, '~' | '!')
}
fn is_quantifier(c: char) -> bool {
    matches!(c, 'A' | 'E' | '∀' | '∃')
}
fn is_ws(c: char) -> bool {
    matches!(c, '\t' | ' ' | '\x0b' | '\n')
}

struct Tnt<'t> {
    text: &'t str,
    cur: Vec<Tok>,
    depth: u32,
    region: Region,
}

impl Tnt<'_> {
    /// `text[i]`; `Err(Index)` past the end.
    fn ch(&self, i: usize) -> Result<char, E> {
        self.text.get(i..).and_then(|s| s.chars().next()).ok_or(E::Index)
    }

    fn push(&mut self, ty: TokenType, s: usize, e: usize) {
        self.cur.push(Tok::new(ty, s, e));
    }

    fn whitespace(&mut self, start: usize) -> usize {
        let mut end = start;
        while let Ok(c) = self.ch(end) {
            if !is_ws(c) {
                break;
            }
            end += 1;
        }
        if end != start {
            self.push(TokenType::TEXT, start, end);
        }
        end
    }

    fn whitespace_required(&mut self, start: usize) -> R {
        let end = self.whitespace(start);
        if end == start { Err(E::Assert) } else { Ok(end) }
    }

    fn variable(&mut self, start: usize) -> R {
        if !is_variable(self.ch(start)?) {
            return Err(E::Assert);
        }
        let mut end = start + 1;
        loop {
            let c = self.ch(end)?;
            if !is_prime(c) {
                break;
            }
            end += c.len_utf8();
        }
        self.push(TokenType::NAME_VARIABLE, start, end);
        Ok(end)
    }

    fn term(&mut self, start: usize) -> R {
        self.depth += 1;
        let r = if self.depth > MAX_DEPTH { Err(E::Assert) } else { self.term_inner(start) };
        self.depth -= 1;
        r
    }

    fn term_inner(&mut self, start: usize) -> R {
        let c = self.ch(start)?;
        if c == 'S' {
            let mut end = start + 1;
            while self.ch(end)? == 'S' {
                end += 1;
            }
            self.push(TokenType::LITERAL_NUMBER_INTEGER, start, end);
            return self.term(end);
        }
        if c == '0' {
            self.push(TokenType::LITERAL_NUMBER_INTEGER, start, start + 1);
            return Ok(start + 1);
        }
        if is_variable(c) {
            return self.variable(start);
        }
        if c == '(' {
            self.push(TokenType::PUNCTUATION, start, start + 1);
            let s = self.term(start + 1)?;
            let op = self.ch(s)?;
            if !is_operator(op) {
                return Err(E::Assert);
            }
            self.push(TokenType::OPERATOR, s, s + op.len_utf8());
            let s = self.term(s + op.len_utf8())?;
            if self.ch(s)? != ')' {
                return Err(E::Assert);
            }
            self.push(TokenType::PUNCTUATION, s, s + 1);
            return Ok(s + 1);
        }
        Err(E::Assert)
    }

    fn formula(&mut self, start: usize) -> R {
        self.depth += 1;
        let r = if self.depth > MAX_DEPTH { Err(E::Assert) } else { self.formula_inner(start) };
        self.depth -= 1;
        r
    }

    fn formula_inner(&mut self, start: usize) -> R {
        let c = self.ch(start)?;
        if is_negator(c) {
            let mut end = start + 1;
            while is_negator(self.ch(end)?) {
                end += 1;
            }
            self.push(TokenType::OPERATOR, start, end);
            return self.formula(end);
        }
        if is_quantifier(c) {
            let n = c.len_utf8();
            self.push(TokenType::KEYWORD_DECLARATION, start, start + n);
            let s = self.variable(start + n)?;
            if self.ch(s)? != ':' {
                return Err(E::Assert);
            }
            self.push(TokenType::PUNCTUATION, s, s + 1);
            return self.formula(s + 1);
        }
        if c == '<' {
            self.push(TokenType::PUNCTUATION, start, start + 1);
            let s = self.formula(start + 1)?;
            let op = self.ch(s)?;
            if !is_logic(op) {
                return Err(E::Assert);
            }
            self.push(TokenType::OPERATOR, s, s + op.len_utf8());
            let s = self.formula(s + op.len_utf8())?;
            if self.ch(s)? != '>' {
                return Err(E::Assert);
            }
            self.push(TokenType::PUNCTUATION, s, s + 1);
            return Ok(s + 1);
        }
        let s = self.term(start)?;
        if self.ch(s)? != '=' {
            return Err(E::Assert);
        }
        self.push(TokenType::OPERATOR, s, s + 1);
        self.term(s + 1)
    }

    fn rule(&mut self, start: usize) -> R {
        self.region.clear();
        let Some(len) =
            RULES.match_with_options(self.text, start, SearchOptions::SEARCH_OPTION_NONE, Some(&mut self.region))
        else {
            return Err(E::Assert);
        };
        let mend = start + len;
        let group = (1..=2).filter_map(|i| self.region.pos(i)).min();
        match group {
            Some((gs, ge)) => {
                self.push(TokenType::KEYWORD, start, gs);
                self.push(TokenType::LITERAL_NUMBER_INTEGER, gs, ge);
                if ge != mend {
                    self.push(TokenType::KEYWORD, ge, mend);
                }
            }
            None => self.push(TokenType::KEYWORD, start, mend),
        }
        Ok(mend)
    }

    fn lineno(&mut self, start: usize) -> R {
        let mut end = start;
        loop {
            let c = self.ch(end)?;
            if c.is_ascii_digit() {
                break;
            }
            end += c.len_utf8();
        }
        self.push(TokenType::PUNCTUATION, start, start + 1);
        self.push(TokenType::TEXT, start + 1, end);
        let Some(len) = match_at(&LINENOS, self.text, end) else { return Err(E::Assert) };
        let mend = end + len;
        if self.ch(mend)? != ')' {
            return Err(E::Assert);
        }
        self.push(TokenType::LITERAL_NUMBER_INTEGER, end, mend);
        self.push(TokenType::PUNCTUATION, mend, mend + 1);
        Ok(mend + 1)
    }

    fn error_till_line_end(&mut self, start: usize) -> usize {
        let mut end = start;
        loop {
            match self.ch(end) {
                Ok('\n') => break,
                Ok(c) => end += c.len_utf8(),
                Err(_) => {
                    end = self.text.len();
                    break;
                }
            }
        }
        if end != start {
            self.push(TokenType::ERROR, start, end);
        }
        self.whitespace(end)
    }

    /// One iteration of the main loop; `Ok` covers Python's `continue`.
    fn step(&mut self, start: &mut usize, end: &mut usize, orig: &mut Option<usize>) -> Result<(), E> {
        // try line number
        while self.ch(*end)?.is_ascii_digit() {
            *end += 1;
        }
        if *end != *start {
            self.push(TokenType::LITERAL_NUMBER_INTEGER, *start, *end);
            let o = self.cur.len();
            *orig = Some(o);
            match self.whitespace_required(*end) {
                Ok(x) => (*start, *end) = (x, x),
                Err(_) => {
                    self.cur.truncate(o);
                    let x = self.error_till_line_end(*end);
                    (*start, *end) = (x, x);
                    return Ok(());
                }
            }
        }
        // at this point it could be a comment
        if let Some(len) = match_at(&COMMENT, self.text, *start) {
            self.push(TokenType::COMMENT, *start, *start + len);
            let x = self.error_till_line_end(*start + len);
            (*start, *end) = (x, x);
            return Ok(());
        }
        let c = self.ch(*start)?;
        if c == '[' || c == ']' {
            self.push(TokenType::KEYWORD, *start, *start + 1);
            *start += 1;
            *end += 1;
        } else {
            let o = self.cur.len();
            *orig = Some(o);
            match self.formula(*start) {
                Ok(x) => (*start, *end) = (x, x),
                Err(E::Assert) => {
                    self.cur.truncate(o);
                    loop {
                        let c = self.ch(*end)?;
                        if is_ws(c) {
                            break;
                        }
                        *end += c.len_utf8();
                    }
                    self.push(TokenType::ERROR, *start, *end);
                    *start = *end;
                }
                Err(E::Index) => return Err(E::Index),
            }
        }
        // skip whitespace after formula
        let o = self.cur.len();
        *orig = Some(o);
        match self.whitespace_required(*end) {
            Ok(x) => (*start, *end) = (x, x),
            Err(_) => {
                self.cur.truncate(o);
                let x = self.error_till_line_end(*start);
                (*start, *end) = (x, x);
                return Ok(());
            }
        }
        // rule proving this formula a theorem
        let o = self.cur.len();
        *orig = Some(o);
        match self.rule(*start) {
            Ok(x) => (*start, *end) = (x, x),
            Err(_) => {
                self.cur.truncate(o);
                let x = self.error_till_line_end(*start);
                (*start, *end) = (x, x);
                return Ok(());
            }
        }
        // skip whitespace after rule
        let x = self.whitespace(*end);
        (*start, *end) = (x, x);
        // line marker
        if self.ch(*start)? == '(' {
            let o = self.cur.len();
            *orig = Some(o);
            match self.lineno(*start) {
                Ok(x) => (*start, *end) = (x, x),
                Err(E::Assert) => {
                    self.cur.truncate(o);
                    let x = self.error_till_line_end(*start);
                    (*start, *end) = (x, x);
                    return Ok(());
                }
                Err(E::Index) => return Err(E::Index),
            }
            let x = self.whitespace(*start);
            (*start, *end) = (x, x);
        }
        Ok(())
    }
}

pub(super) fn tnt(_lexer: &'static LexerImpl, text: &str, out: &mut Vec<Tok>) {
    let mut t = Tnt { text, cur: Vec::new(), depth: 0, region: Region::new() };
    let mut start = t.whitespace(0);
    let mut end = start;
    let mut orig = None;
    while start <= end && end < text.len() {
        let before = (start, end);
        if t.step(&mut start, &mut end, &mut orig).is_err() {
            if let Some(o) = orig {
                t.cur.truncate(o);
            }
            t.error_till_line_end(start);
            // Python would retry the same position forever.
            if (start, end) == before {
                break;
            }
        }
    }
    out.append(&mut t.cur);
}

#[cfg(test)]
mod tests {
    use super::super::{exp, lex};

    #[test]
    fn tnt_theorems() {
        assert_eq!(
            lex("tnt", "0 Aa:~<a=0∧Sb'=(a+SS0)> premise\n1 [ fantasy rule\n2 a=b carry over line 1 (lines 1, 2)\n"),
            exp(&[
                ("Literal.Number.Integer", "0"),
                ("Text", " "),
                ("Keyword.Declaration", "A"),
                ("Name.Variable", "a"),
                ("Punctuation", ":"),
                ("Operator", "~"),
                ("Punctuation", "<"),
                ("Name.Variable", "a"),
                ("Operator", "="),
                ("Literal.Number.Integer", "0"),
                ("Operator", "∧"),
                ("Literal.Number.Integer", "S"),
                ("Name.Variable", "b'"),
                ("Operator", "="),
                ("Punctuation", "("),
                ("Name.Variable", "a"),
                ("Operator", "+"),
                ("Literal.Number.Integer", "SS"),
                ("Literal.Number.Integer", "0"),
                ("Punctuation", ")"),
                ("Punctuation", ">"),
                ("Text", " "),
                ("Keyword", "premise"),
                ("Text", "\n"),
                ("Literal.Number.Integer", "1"),
                ("Text", " "),
                ("Keyword", "["),
                ("Text", " "),
                ("Keyword", "fantasy rule"),
                ("Text", "\n"),
                ("Literal.Number.Integer", "2"),
                ("Text", " "),
                ("Name.Variable", "a"),
                ("Operator", "="),
                ("Name.Variable", "b"),
                ("Text", " "),
                ("Keyword", "carry over line "),
                ("Literal.Number.Integer", "1"),
                ("Text", " "),
                ("Punctuation", "("),
                ("Text", "lines "),
                ("Literal.Number.Integer", "1, 2"),
                ("Punctuation", ")"),
                ("Text", "\n"),
            ])
        );
    }

    #[test]
    fn tnt_errors() {
        assert_eq!(
            lex("tnt", "0=0 premise (1)\nxyz junk\n[comment] x\n3 a=b bogus\n"),
            exp(&[
                ("Literal.Number.Integer", "0"),
                ("Error", "=0 premise (1)"),
                ("Text", "\n"),
                ("Error", "xyz"),
                ("Text", " "),
                ("Error", "junk"),
                ("Text", "\n"),
                ("Comment", "[comment]"),
                ("Error", " x"),
                ("Text", "\n"),
                ("Literal.Number.Integer", "3"),
                ("Text", " "),
                ("Name.Variable", "a"),
                ("Operator", "="),
                ("Name.Variable", "b"),
                ("Text", " "),
                ("Error", "bogus"),
                ("Text", "\n"),
            ])
        );
    }
}
