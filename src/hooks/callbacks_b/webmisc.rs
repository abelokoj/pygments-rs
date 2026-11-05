//! `XQueryLexer` callbacks. They juggle the state stack and a second stack of
//! saved states (`xquery_parse_state`, a class attribute in Python; kept per
//! tokenization in [`Extra`](crate::hooks::Extra) here so resuming works).

use serde_json::Value;

use crate::engine::StateId;
use crate::hooks::{Cx, Groups};
use crate::token::TokenType;

#[derive(Clone, Debug, Default, PartialEq)]
struct ParseState(Vec<StateId>);

fn saved(cx: &mut Cx<'_>) -> &mut Vec<StateId> {
    &mut cx.extra.get::<ParseState>().0
}

/// `lexer.xquery_parse_state.append(name)`
fn save(cx: &mut Cx<'_>, name: &str) {
    let id = cx.state(name);
    saved(cx).push(id);
}

/// `ctx.stack = ['root']`
fn to_root(cx: &mut Cx<'_>) {
    let root = cx.state("root");
    cx.stack.clear();
    cx.stack.push(root);
}

fn group(cx: &mut Cx<'_>, g: &Groups<'_>, i: usize, ty: TokenType) {
    if let Some((s, e)) = g.get(i) {
        cx.emit(ty, s, e);
    }
}

/// Groups 1-3 as `first`, whitespace, `third`.
fn three(cx: &mut Cx<'_>, g: &Groups<'_>, first: TokenType, third: TokenType) {
    group(cx, g, 1, first);
    group(cx, g, 2, TokenType::TEXT_WHITESPACE);
    group(cx, g, 3, third);
}

pub(super) fn punctuation_root(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    group(cx, g, 1, TokenType::PUNCTUATION);
    to_root(cx);
    cx.pos = g.end();
}

pub(super) fn operator_root(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    group(cx, g, 1, TokenType::OPERATOR);
    to_root(cx);
    cx.pos = g.end();
}

pub(super) fn popstate_tag(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    group(cx, g, 1, TokenType::NAME_TAG);
    if let Some(s) = saved(cx).pop() {
        cx.stack.push(s);
    }
    cx.pos = g.end();
}

pub(super) fn popstate_xmlcomment(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    group(cx, g, 1, TokenType::LITERAL_STRING_DOC);
    // Python raises IndexError on an empty list; we just keep the stack.
    if let Some(s) = saved(cx).pop() {
        cx.stack.push(s);
    }
    cx.pos = g.end();
}

pub(super) fn popstate_kindtest(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    group(cx, g, 1, TokenType::PUNCTUATION);
    let end1 = g.get(1).map_or(g.end(), |(_, e)| e);
    let next = saved(cx).pop();
    if next == Some(cx.state("occurrenceindicator")) {
        let operator = cx.state("operator");
        // Group 2 is `[*+?]?`, so `re.match("[?*+]+", ...)` holds iff it is non-empty.
        match g.get(2).filter(|(s, e)| e > s) {
            Some((s, e)) => {
                cx.emit(TokenType::PUNCTUATION, s, e);
                cx.stack.push(operator);
                cx.pos = g.end();
            }
            None => {
                cx.stack.push(operator);
                cx.pos = end1;
            }
        }
    } else {
        if let Some(s) = next {
            cx.stack.push(s);
        }
        cx.pos = end1;
    }
}

pub(super) fn popstate(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    group(cx, g, 1, TokenType::PUNCTUATION);
    if saved(cx).is_empty() {
        cx.stack.pop();
        if cx.stack.is_empty() {
            to_root(cx);
        }
    } else if cx.stack.len() > 1 {
        if let Some(s) = saved(cx).pop() {
            cx.stack.push(s);
        }
    } else {
        to_root(cx);
    }
    cx.pos = g.end();
}

/// Emit group 1 as `ty`, push `state` and save `next`.
fn push1(cx: &mut Cx<'_>, g: &Groups<'_>, ty: TokenType, state: &str, next: &str) {
    group(cx, g, 1, ty);
    cx.push_state(state);
    save(cx, next);
    cx.pos = g.end();
}

pub(super) fn element_content_starttag(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    push1(cx, g, TokenType::NAME_TAG, "start_tag", "element_content");
}

pub(super) fn element_content_processing_instruction(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    push1(cx, g, TokenType::LITERAL_STRING_DOC, "processing_instruction", "element_content");
}

pub(super) fn element_content_cdata_section(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    push1(cx, g, TokenType::LITERAL_STRING_DOC, "cdata_section", "element_content");
}

pub(super) fn element_content_xmlcomment(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    push1(cx, g, TokenType::LITERAL_STRING_DOC, "xml_comment", "element_content");
}

pub(super) fn operator_processing_instruction(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    push1(cx, g, TokenType::LITERAL_STRING_DOC, "processing_instruction", "operator");
}

pub(super) fn operator_cdata_section(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    push1(cx, g, TokenType::LITERAL_STRING_DOC, "cdata_section", "operator");
}

pub(super) fn operator_xmlcomment(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    push1(cx, g, TokenType::LITERAL_STRING_DOC, "xml_comment", "operator");
}

pub(super) fn operator_starttag(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    push1(cx, g, TokenType::NAME_TAG, "start_tag", "operator");
}

/// Emit groups 1-3, push `state` and save `next`.
fn push3(cx: &mut Cx<'_>, g: &Groups<'_>, first: TokenType, state: &str, next: &str) {
    three(cx, g, first, TokenType::PUNCTUATION);
    save(cx, next);
    cx.push_state(state);
    cx.pos = g.end();
}

pub(super) fn kindtest(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    push3(cx, g, TokenType::KEYWORD, "kindtest", "kindtest");
}

pub(super) fn operator_kindtestforpi(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    push3(cx, g, TokenType::KEYWORD, "kindtestforpi", "operator");
}

pub(super) fn operator_kindtest(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    push3(cx, g, TokenType::KEYWORD, "kindtest", "operator");
}

pub(super) fn occurrenceindicator_kindtest(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    push3(cx, g, TokenType::NAME_TAG, "kindtest", "occurrenceindicator");
}

/// Emit groups 1-3, reset to root and save `operator`.
fn root3(cx: &mut Cx<'_>, g: &Groups<'_>, third: TokenType) {
    three(cx, g, TokenType::KEYWORD, third);
    to_root(cx);
    save(cx, "operator");
    cx.pos = g.end();
}

pub(super) fn operator_order(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    root3(cx, g, TokenType::PUNCTUATION);
}

pub(super) fn operator_map(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    root3(cx, g, TokenType::PUNCTUATION);
}

pub(super) fn operator_root_validate(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    root3(cx, g, TokenType::PUNCTUATION);
}

pub(super) fn operator_root_validate_withmode(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    root3(cx, g, TokenType::KEYWORD);
}

pub(super) fn operator_root_construct(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    root3(cx, g, TokenType::PUNCTUATION);
}

pub(super) fn push_operator_root(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    group(cx, g, 1, TokenType::PUNCTUATION);
    save(cx, "operator");
    to_root(cx);
    cx.pos = g.end();
}

pub(super) fn root(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    group(cx, g, 1, TokenType::PUNCTUATION);
    if let Some(cur) = cx.stack.pop() {
        saved(cx).push(cur);
    }
    to_root(cx);
    cx.pos = g.end();
}
