//! `pygments.lexers.perl.Perl6Lexer` callbacks.

use std::sync::LazyLock;

use onig::Regex;
use serde_json::Value;

use super::{arg_ty, find_from, fwd_chars, re_escape};
use crate::engine::compile;
use crate::hooks::{Cx, Groups};
use crate::token::TokenType;

/// `ctx.perl6_token_nesting_level`.
#[derive(Clone, Debug, Default, PartialEq)]
struct Nesting(i64);

static TO_RE: LazyLock<Regex> = LazyLock::new(|| compile(r":to\b", "").expect("valid regex"));

pub(super) fn brackets(cx: &mut Cx<'_>, m: &Groups<'_>, args: &Value) {
    let text = cx.text;
    // Named groups: `delimiter` is 1, or 2 after `adverbs` when the rule has three groups.
    let (delim, adverbs) = if m.get(3).is_some() { (m.get(2), m.str(text, 1)) } else { (m.get(1), None) };
    let Some((ds, de)) = delim else {
        cx.pos = m.end();
        return;
    };
    let opening = &text[ds..de];
    let n_chars = opening.chars().count();
    let first = opening.chars().next().map_or("", |c| &opening[..c.len_utf8()]);
    let closer = cx.lexer.konst("PERL6_BRACKETS").and_then(|v| v.get(first)).and_then(Value::as_str);

    let mut end_pos = match closer {
        None => find_from(text, opening, de),
        Some(c) => {
            let closing = c.repeat(n_chars);
            let mut nesting = 1;
            let mut search = ds;
            let mut next_close = text.len();
            while nesting > 0 {
                let from = fwd_chars(text, search, n_chars);
                let next_open = find_from(text, opening, from);
                match find_from(text, &closing, from) {
                    None => {
                        next_close = text.len();
                        nesting = 0;
                    }
                    Some(close) => {
                        next_close = close;
                        match next_open {
                            Some(open) if open < close => {
                                nesting += 1;
                                search = open;
                            }
                            _ => {
                                nesting -= 1;
                                search = close;
                            }
                        }
                    }
                }
            }
            Some(next_close)
        }
    }
    .unwrap_or(text.len());

    if adverbs.is_some_and(|a| TO_RE.find(a).is_some()) {
        let terminator = text.get(de..end_pos).unwrap_or("");
        let found =
            compile(&format!(r"^\s*{}\s*$", re_escape(terminator)), "m").ok().and_then(|re| re.find(&text[end_pos..]));
        end_pos = match found {
            Some((_, e)) => end_pos + e,
            None => text.len(),
        };
    }

    let fin = fwd_chars(text, end_pos, n_chars);
    cx.emit(arg_ty(args, "token_class", ""), m.start(), fin);
    cx.pos = fin;
}

fn below_token(cx: &Cx<'_>) -> bool {
    let n = cx.stack.len();
    n > 2 && cx.stack[n - 2] == cx.state("token")
}

pub(super) fn opening_brace(cx: &mut Cx<'_>, m: &Groups<'_>, _: &Value) {
    cx.emit(TokenType::TEXT, m.start(), m.end());
    cx.pos = m.end();
    if below_token(cx) {
        cx.extra.get::<Nesting>().0 += 1;
    }
}

pub(super) fn closing_brace(cx: &mut Cx<'_>, m: &Groups<'_>, _: &Value) {
    cx.emit(TokenType::TEXT, m.start(), m.end());
    cx.pos = m.end();
    if below_token(cx) {
        let level = &mut cx.extra.get::<Nesting>().0;
        *level -= 1;
        if *level == 0 {
            cx.stack.pop();
        }
    }
}

pub(super) fn embedded_perl6(cx: &mut Cx<'_>, m: &Groups<'_>, _: &Value) {
    cx.extra.get::<Nesting>().0 = 1;
    cx.emit(TokenType::TEXT, m.start(), m.end());
    cx.pos = m.end();
    cx.push_state("root");
}
