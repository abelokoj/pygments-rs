//! `pygments.lexers.mime.MIMELexer` callbacks. The instance attributes the
//! callbacks set (`content_type`, `boundary`, ...) live in `Extra`.

use onig::{Region, SearchOptions};
use serde_json::Value;

use super::{emit_g, lex_shared, py_strip, re_escape};
use crate::engine::compile;
use crate::hooks::{Cx, Groups, lex_sub};
use crate::token::TokenType;

#[derive(Clone, Debug, PartialEq)]
struct Mime {
    content_type: String,
    boundary: Option<String>,
    content_transfer_encoding: Option<String>,
}

impl Default for Mime {
    fn default() -> Mime {
        Mime { content_type: "text/plain".to_owned(), boundary: None, content_transfer_encoding: None }
    }
}

pub(super) fn get_header_tokens(cx: &mut Cx<'_>, m: &Groups<'_>, _: &Value) {
    let (Some((s1, e1)), Some((s3, e3))) = (m.get(1), m.get(3)) else { return };
    let field = cx.text[s1..e1].to_lowercase();
    if cx.lexer.word_set("attention_headers").contains(field.as_str()) {
        // `field + ":"`: the colon right after group 1.
        cx.emit(TokenType::NAME_TAG, s1, e1 + 1);
        emit_g(cx, m, 2, TokenType::TEXT_WHITESPACE);
        lex_shared(cx, s3, e3, &["root", field.as_str()]);
    } else {
        cx.emit(TokenType::COMMENT, m.start(), m.end());
    }
}

pub(super) fn get_body_tokens(cx: &mut Cx<'_>, m: &Groups<'_>, _: &Value) {
    let text = cx.text;
    let (mut ps, e) = (m.start(), m.end());
    if text[ps..e].starts_with('\n') {
        cx.emit(TokenType::TEXT_WHITESPACE, ps, ps + 1);
        ps += 1;
    }
    let st = cx.extra.get::<Mime>().clone();
    let boundary = st.boundary.as_deref().filter(|b| !b.is_empty() && st.content_type.starts_with("multipart"));
    let Some(re) = boundary.and_then(|b| compile(&format!("^--{}(--)?\\n", re_escape(b)), "m").ok()) else {
        bodypart(cx, &st, ps, e);
        return;
    };
    let body = &text[ps..e];
    let mut region = Region::new();
    let search = |from: usize, region: &mut Region| {
        re.search_with_options(body, from, body.len(), SearchOptions::SEARCH_OPTION_NONE, Some(&mut *region))
            .and_then(|_| region.pos(0))
    };
    let (mut part_start, mut from) = match search(0, &mut region) {
        Some((ms, me)) => {
            cx.emit(TokenType::TEXT, ps, ps + ms);
            cx.emit(TokenType::LITERAL_STRING_DELIMITER, ps + ms, ps + me);
            (ps + me, me)
        }
        None => (ps, 0),
    };
    while let Some((ms, me)) = search(from, &mut region) {
        bodypart(cx, &st, part_start, ps + ms);
        cx.emit(TokenType::LITERAL_STRING_DELIMITER, ps + ms, ps + me);
        part_start = ps + me;
        from = me;
    }
    if part_start != e {
        cx.emit(TokenType::TEXT, part_start, e);
    }
}

/// `get_bodypart_tokens` for `text[s..e]`.
fn bodypart(cx: &mut Cx<'_>, st: &Mime, s: usize, e: usize) {
    let readable = st
        .content_transfer_encoding
        .as_deref()
        .is_none_or(|c| c.is_empty() || matches!(c, "8bit" | "7bit" | "quoted-printable"));
    // `max_nested_level` defaults to -1 and only decreases, so it never stops recursion.
    let lexer = if py_strip(&cx.text[s..e]).is_empty() || st.content_type.is_empty() || !readable {
        None
    } else {
        crate::registry::lexer_for_mimetype(&st.content_type)
    };
    match lexer {
        Some(l) => lex_sub(l.imp(), cx.text, s, e, None, cx.out),
        None => cx.emit(TokenType::OTHER, s, e),
    }
}

pub(super) fn store_content_type(cx: &mut Cx<'_>, m: &Groups<'_>, _: &Value) {
    let (Some((s1, e1)), Some((s2, e2))) = (m.get(1), m.get(2)) else { return };
    cx.extra.get::<Mime>().content_type = cx.text[s1..e1].to_owned();
    cx.emit(TokenType::TEXT_WHITESPACE, m.start(), s1);
    cx.emit(TokenType::NAME_LABEL, s2, e2);
    cx.emit(TokenType::LITERAL_STRING_DELIMITER, e2, e2 + 1);
    emit_g(cx, m, 3, TokenType::NAME_LABEL);
}

pub(super) fn get_content_type_subtokens(cx: &mut Cx<'_>, m: &Groups<'_>, _: &Value) {
    let text = cx.text;
    emit_g(cx, m, 1, TokenType::TEXT);
    emit_g(cx, m, 2, TokenType::TEXT_WHITESPACE);
    emit_g(cx, m, 3, TokenType::NAME_ATTRIBUTE);
    emit_g(cx, m, 4, TokenType::OPERATOR);
    emit_g(cx, m, 5, TokenType::LITERAL_STRING);
    if m.str(text, 3).is_some_and(|a| a.eq_ignore_ascii_case("boundary")) {
        let mut b = py_strip(m.str(text, 5).unwrap_or(""));
        if b.starts_with('"') && b.ends_with('"') {
            b = b.get(1..b.len() - 1).unwrap_or("");
        }
        cx.extra.get::<Mime>().boundary = Some(b.to_owned());
    }
}

pub(super) fn store_content_transfer_encoding(cx: &mut Cx<'_>, m: &Groups<'_>, _: &Value) {
    let (s, e) = (m.start(), m.end());
    cx.extra.get::<Mime>().content_transfer_encoding = Some(cx.text[s..e].to_lowercase());
    cx.emit(TokenType::NAME_CONSTANT, s, e);
}
