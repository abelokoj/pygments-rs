//! `pygments.lexers.data.YamlLexer` callbacks; `YamlLexerContext` lives in `Extra`.

use serde_json::Value;

use super::{arg_bool, arg_ty};
use crate::hooks::{Cx, Groups};

#[derive(Clone, Debug, PartialEq)]
struct YamlCtx {
    indent_stack: Vec<i64>,
    indent: i64,
    next_indent: i64,
    block_scalar_indent: Option<i64>,
}

impl Default for YamlCtx {
    fn default() -> YamlCtx {
        YamlCtx { indent_stack: Vec::new(), indent: -1, next_indent: 0, block_scalar_indent: None }
    }
}

/// Python slice index `i` into a sequence of length `len` (negative counts from the end).
fn py_index(i: i64, len: usize) -> usize {
    if i < 0 { len.saturating_sub(i.unsigned_abs() as usize) } else { (i as usize).min(len) }
}

fn pop2(cx: &mut Cx<'_>) {
    cx.stack.pop();
    cx.stack.pop();
}

pub(super) fn something(cx: &mut Cx<'_>, m: &Groups<'_>, args: &Value) {
    let (s, e) = (m.start(), m.end());
    if s == e {
        return;
    }
    cx.emit(arg_ty(args, "token_class", ""), s, e);
    cx.pos = e;
}

pub(super) fn reset_indent(cx: &mut Cx<'_>, m: &Groups<'_>, args: &Value) {
    *cx.extra.get::<YamlCtx>() = YamlCtx::default();
    cx.emit(arg_ty(args, "token_class", ""), m.start(), m.end());
    cx.pos = m.end();
}

pub(super) fn save_indent(cx: &mut Cx<'_>, m: &Groups<'_>, args: &Value) {
    let (s, e) = (m.start(), m.end());
    let mut text_len = e - s;
    let mut extra_len = 0;
    let st = cx.extra.get::<YamlCtx>();
    if arg_bool(args, "start") {
        st.next_indent = text_len as i64;
        if st.next_indent < st.indent {
            while st.next_indent < st.indent {
                st.indent = st.indent_stack.pop().unwrap_or(-1);
            }
            if st.next_indent > st.indent {
                let cut = py_index(st.indent, text_len);
                extra_len = text_len - cut;
                text_len = cut;
            }
        }
    } else {
        st.next_indent += text_len as i64;
    }
    if text_len > 0 {
        cx.emit(arg_ty(args, "token_class", ""), s, s + text_len);
    }
    if extra_len > 0 {
        cx.emit(arg_ty(args, "token_class", ".Error"), s + text_len, e);
    }
    cx.pos = e;
}

pub(super) fn set_indent(cx: &mut Cx<'_>, m: &Groups<'_>, args: &Value) {
    let (s, e) = (m.start(), m.end());
    let len = cx.text[s..e].chars().count() as i64;
    let st = cx.extra.get::<YamlCtx>();
    if st.indent < st.next_indent {
        st.indent_stack.push(st.indent);
        st.indent = st.next_indent;
    }
    if !arg_bool(args, "implicit") {
        st.next_indent += len;
    }
    cx.emit(arg_ty(args, "token_class", ""), s, e);
    cx.pos = e;
}

pub(super) fn set_block_scalar_indent(cx: &mut Cx<'_>, m: &Groups<'_>, args: &Value) {
    let (s, e) = (m.start(), m.end());
    let increment = m.str(cx.text, 1).and_then(|g| g.parse::<i64>().ok());
    let st = cx.extra.get::<YamlCtx>();
    st.block_scalar_indent = None;
    if s == e {
        return;
    }
    if let Some(inc) = increment {
        st.block_scalar_indent = Some(st.indent.max(0) + inc);
    }
    cx.emit(arg_ty(args, "token_class", ""), s, e);
    cx.pos = e;
}

pub(super) fn parse_block_scalar_empty_line(cx: &mut Cx<'_>, m: &Groups<'_>, args: &Value) {
    let (s, e) = (m.start(), m.end());
    let indent_ty = arg_ty(args, "indent_token_class", "");
    match cx.extra.get::<YamlCtx>().block_scalar_indent {
        Some(b) if (e - s) as i64 > b => {
            let mid = s + b as usize;
            cx.emit(indent_ty, s, mid);
            cx.emit(arg_ty(args, "content_token_class", ""), mid, e);
        }
        _ => {
            if e > s {
                cx.emit(indent_ty, s, e);
            }
        }
    }
    cx.pos = e;
}

pub(super) fn parse_block_scalar_indent(cx: &mut Cx<'_>, m: &Groups<'_>, args: &Value) {
    let (s, e) = (m.start(), m.end());
    let len = (e - s) as i64;
    let st = cx.extra.get::<YamlCtx>();
    match st.block_scalar_indent {
        None => {
            if len <= st.indent.max(0) {
                pop2(cx);
                return;
            }
            st.block_scalar_indent = Some(len);
        }
        Some(b) => {
            if len < b {
                pop2(cx);
                return;
            }
        }
    }
    if e > s {
        cx.emit(arg_ty(args, "token_class", ""), s, e);
        cx.pos = e;
    }
}

pub(super) fn parse_plain_scalar_indent(cx: &mut Cx<'_>, m: &Groups<'_>, args: &Value) {
    let (s, e) = (m.start(), m.end());
    if (e - s) as i64 <= cx.extra.get::<YamlCtx>().indent {
        pop2(cx);
        return;
    }
    if e > s {
        cx.emit(arg_ty(args, "token_class", ""), s, e);
        cx.pos = e;
    }
}
