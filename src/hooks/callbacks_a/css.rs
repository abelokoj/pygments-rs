//! `pygments.lexers.css._indentation` / `_starts_block` (Sass, Haml, Scaml,
//! Pug, Slim): indentation-delimited blocks, state kept in `Extra`.

use serde_json::Value;

use super::arg_ty;
use crate::engine::StateId;
use crate::hooks::{Cx, Groups};
use crate::token::TokenType;

#[derive(Clone, Debug, Default, PartialEq)]
struct Indent {
    last_indentation: Option<String>,
    block_state: Option<StateId>,
    block_indentation: Option<String>,
}

pub(super) fn indentation(cx: &mut Cx<'_>, m: &Groups<'_>, _: &Value) {
    let (s, e) = (m.start(), m.end());
    let text = cx.text;
    let ind = &text[s..e];
    cx.emit(TokenType::TEXT_WHITESPACE, s, e);
    cx.pos = e;
    let content = cx.state("content");
    let st = cx.extra.get::<Indent>();
    let last = st.last_indentation.get_or_insert_with(String::new);
    last.clear();
    last.push_str(ind);
    let keep = match (st.block_state, st.block_indentation.as_deref()) {
        (Some(b), Some(bi)) if ind.starts_with(bi) && ind != bi => Some(b),
        _ => None,
    };
    if keep.is_none() {
        st.block_state = None;
        st.block_indentation = None;
    }
    cx.stack.push(keep.unwrap_or(content));
}

pub(super) fn starts_block(cx: &mut Cx<'_>, m: &Groups<'_>, args: &Value) {
    cx.emit(arg_ty(args, "token", ""), m.start(), m.end());
    let state = args.get("state").and_then(Value::as_str).map(|n| cx.state(n));
    let Indent { last_indentation, block_state, block_indentation } = cx.extra.get::<Indent>();
    let bi = block_indentation.get_or_insert_with(String::new);
    bi.clear();
    bi.push_str(last_indentation.as_deref().unwrap_or(""));
    *block_state = state;
    cx.pos = m.end();
}
