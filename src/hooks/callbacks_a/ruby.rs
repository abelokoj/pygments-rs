//! Heredocs of Ruby, Crystal and Terraform, and Ruby's `%r`/`%Q` strings.

use serde_json::Value;

use super::{emit_g, py_rstrip, py_strip};
use crate::engine::Run;
use crate::hooks::{Cx, Groups};
use crate::token::TokenType;

/// Pending heredocs `(tolerant, name start, name end)`: Pygments' `ctx.heredocstack`.
#[derive(Clone, Debug, Default, PartialEq)]
struct Heredocs(Vec<(bool, usize, usize)>);

pub(super) fn ruby_heredoc(cx: &mut Cx<'_>, m: &Groups<'_>, _: &Value) {
    heredoc(cx, m, &["<<-", "<<~"]);
}

pub(super) fn crystal_heredoc(cx: &mut Cx<'_>, m: &Groups<'_>, _: &Value) {
    heredoc(cx, m, &["<<-"]);
}

fn heredoc(cx: &mut Cx<'_>, m: &Groups<'_>, tolerant_ops: &[&str]) {
    let text = cx.text;
    let (Some(g1), Some(g3), Some(g5)) = (m.get(1), m.get(3), m.get(5)) else {
        cx.pos = m.end();
        return;
    };
    cx.emit(TokenType::OPERATOR, g1.0, g1.1);
    emit_g(cx, m, 2, TokenType::LITERAL_STRING_HEREDOC);
    cx.emit(TokenType::LITERAL_STRING_DELIMITER, g3.0, g3.1);
    emit_g(cx, m, 4, TokenType::LITERAL_STRING_HEREDOC);

    let tolerant = tolerant_ops.contains(&&text[g1.0..g1.1]);
    let stack = &mut cx.extra.get::<Heredocs>().0;
    let outermost = stack.is_empty();
    stack.push((tolerant, g3.0, g3.1));
    if stack.len() < 100 {
        // `self.get_tokens_unprocessed(context=ctx)` with `ctx.end = match.end(5)`:
        // the same stack and heredoc list, the text cut at the end of the line.
        let mut run = Run::new(cx.lexer, &text[..g5.1], g5.0, std::mem::take(cx.stack), std::mem::take(cx.extra));
        while run.step() {
            cx.out.extend(run.out.drain(..));
        }
        cx.out.extend(run.out.drain(..));
        *cx.stack = run.stack;
        *cx.extra = run.extra;
    } else {
        cx.emit(TokenType::LITERAL_STRING_HEREDOC, g5.0, g5.1);
    }
    cx.pos = m.end();

    if outermost {
        let pending = std::mem::take(&mut cx.extra.get::<Heredocs>().0);
        for (tolerant, ns, ne) in pending {
            scan_body(cx, tolerant, &text[ns..ne]);
        }
    }
}

pub(super) fn terraform_heredoc(cx: &mut Cx<'_>, m: &Groups<'_>, _: &Value) {
    let text = cx.text;
    emit_g(cx, m, 1, TokenType::OPERATOR);
    emit_g(cx, m, 2, TokenType::LITERAL_STRING_DELIMITER);
    emit_g(cx, m, 3, TokenType::LITERAL_STRING_HEREDOC);
    cx.pos = m.end();
    if let Some(name) = m.str(text, 2) {
        scan_body(cx, true, name);
    }
}

/// Emit the heredoc lines after `cx.pos` up to the line equal to `name`; if it
/// never comes, emit the lines as errors and leave `cx.pos` (Pygments re-lexes them).
fn scan_body(cx: &mut Cx<'_>, tolerant: bool, name: &str) {
    let text = cx.text;
    let begin = cx.pos;
    let mut p = begin;
    // `line_re = .*?\n`: only newline-terminated lines.
    while let Some(nl) = text[p..].find('\n') {
        let le = p + nl + 1;
        let line = &text[p..le];
        let check = if tolerant { py_strip(line) } else { py_rstrip(line) };
        if check == name {
            emit_lines(cx, begin, p, TokenType::LITERAL_STRING_HEREDOC);
            cx.emit(TokenType::LITERAL_STRING_DELIMITER, p, le);
            cx.pos = le;
            return;
        }
        p = le;
    }
    emit_lines(cx, begin, p, TokenType::ERROR);
}

fn emit_lines(cx: &mut Cx<'_>, mut p: usize, end: usize, ty: TokenType) {
    while p < end {
        let le = cx.text[p..end].find('\n').map_or(end, |i| p + i + 1);
        cx.emit(ty, p, le);
        p = le;
    }
}

pub(super) fn intp_regex(cx: &mut Cx<'_>, m: &Groups<'_>, _: &Value) {
    intp(cx, m, TokenType::LITERAL_STRING_REGEX, "interpolated-regex");
}

pub(super) fn intp_string(cx: &mut Cx<'_>, m: &Groups<'_>, _: &Value) {
    intp(cx, m, TokenType::LITERAL_STRING_OTHER, "interpolated-string");
}

fn intp(cx: &mut Cx<'_>, m: &Groups<'_>, ty: TokenType, state: &str) {
    emit_g(cx, m, 1, ty);
    if let Some((s, e)) = m.get(3) {
        cx.lex_this(s, e, Some(&[state]));
    }
    emit_g(cx, m, 4, ty);
    cx.pos = m.end();
}
