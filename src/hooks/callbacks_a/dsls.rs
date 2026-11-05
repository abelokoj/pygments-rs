//! `pygments.lexers.dsls.SnowballLexer` string callbacks.

use serde_json::Value;

use super::arg_bool;
use crate::hooks::{Cx, Groups};
use crate::token::TokenType;

/// The lexer's `_start` / `_end` string-escape characters.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Escapes {
    start: char,
    end: char,
}

impl Default for Escapes {
    fn default() -> Escapes {
        Escapes { start: '\'', end: '\'' }
    }
}

pub(super) fn stringescapes(cx: &mut Cx<'_>, m: &Groups<'_>, _: &Value) {
    let text = cx.text;
    let first = |i| m.str(text, i).and_then(|s| s.chars().next());
    if let (Some(start), Some(end)) = (first(3), first(5)) {
        *cx.extra.get::<Escapes>() = Escapes { start, end };
    }
    let types = [
        TokenType::KEYWORD_RESERVED,
        TokenType::TEXT_WHITESPACE,
        TokenType::LITERAL_STRING_ESCAPE,
        TokenType::TEXT_WHITESPACE,
        TokenType::LITERAL_STRING_ESCAPE,
    ];
    for (i, ty) in types.into_iter().enumerate() {
        if let Some((s, e)) = m.get(i + 1).filter(|(s, e)| e > s) {
            cx.emit(ty, s, e);
        }
    }
    cx.pos = m.end();
}

/// `re.compile('([^c]*)(.)').match(text, pos)`: end of group 1 and of group 2.
fn upto(text: &str, pos: usize, c: char) -> Option<(usize, usize)> {
    let rest = &text[pos..];
    let run = rest.find(c).unwrap_or(rest.len());
    if run < rest.len() && c != '\n' {
        return Some((pos + run, pos + run + c.len_utf8()));
    }
    // Backtrack: `.` takes the last non-newline character of the run.
    rest[..run].char_indices().rev().find(|&(_, ch)| ch != '\n').map(|(i, ch)| (pos + i, pos + i + ch.len_utf8()))
}

pub(super) fn string(cx: &mut Cx<'_>, m: &Groups<'_>, args: &Value) {
    let s = m.start();
    let full = cx.text;
    let text = &full[s..m.end()];
    let Escapes { start, end } = *cx.extra.get::<Escapes>();
    let escape_state = cx.state("escape");
    let mut pos = 0;
    let mut last_end = text.len();
    let mut do_string = arg_bool(args, "do_string_first");
    while pos < text.len() {
        if do_string {
            let Some((g1, g2)) = upto(text, pos, start) else { break };
            cx.emit(TokenType::LITERAL_STRING_SINGLE, s + pos, s + g1);
            last_end = g2;
            if &text[g1..g2] == "'" {
                cx.emit(TokenType::LITERAL_STRING_SINGLE, s + g1, s + g2);
                cx.stack.pop();
                break;
            }
            cx.emit(TokenType::LITERAL_STRING_ESCAPE, s + g1, s + g2);
            pos = g2;
        }
        let Some((g1, g2)) = upto(text, pos, end) else {
            last_end = text.len();
            break;
        };
        cx.emit(TokenType::LITERAL_STRING_ESCAPE, s + pos, s + g2);
        last_end = g2;
        if !text[g1..g2].starts_with(end) {
            if let Some(top) = cx.stack.last_mut() {
                *top = escape_state;
            }
            break;
        }
        pos = g2;
        do_string = true;
    }
    cx.pos = s + last_end;
}
