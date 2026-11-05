//! `pygments.lexers.textfmts`: `HttpLexer` and `NotmuchLexer` callbacks.

use serde_json::Value;

use super::{by_alias, emit_g, py_strip};
use crate::hooks::{Cx, Groups, lex_sub};
use crate::token::TokenType;

/// `HttpLexer.content_type`.
#[derive(Clone, Debug, Default, PartialEq)]
struct Http {
    content_type: Option<String>,
}

pub(super) fn http_header(cx: &mut Cx<'_>, m: &Groups<'_>, _: &Value) {
    let text = cx.text;
    if m.str(text, 1).is_some_and(|f| f.eq_ignore_ascii_case("content-type")) {
        let mut ct = py_strip(m.str(text, 5).unwrap_or(""));
        if let Some(i) = ct.find(';') {
            ct = py_strip(&ct[..i]);
        }
        cx.extra.get::<Http>().content_type = Some(ct.to_owned());
    }
    let types = [
        TokenType::NAME_ATTRIBUTE,
        TokenType::TEXT,
        TokenType::OPERATOR,
        TokenType::TEXT,
        TokenType::LITERAL,
        TokenType::TEXT,
    ];
    for (i, ty) in types.into_iter().enumerate() {
        emit_g(cx, m, i + 1, ty);
    }
}

pub(super) fn http_continuous_header(cx: &mut Cx<'_>, m: &Groups<'_>, _: &Value) {
    emit_g(cx, m, 1, TokenType::TEXT);
    emit_g(cx, m, 2, TokenType::LITERAL);
    emit_g(cx, m, 3, TokenType::TEXT);
}

pub(super) fn http_content(cx: &mut Cx<'_>, m: &Groups<'_>, _: &Value) {
    let (s, e) = (m.start(), m.end());
    let content_type = cx.extra.get::<Http>().content_type.clone().unwrap_or_default();
    if !content_type.is_empty() {
        // `re.sub(r'^(.*)/.*\+(.*)$', r'\1/\2', ct)`: `application/calendar+xml` -> `application/xml`.
        let general = content_type.rfind('+').and_then(|plus| {
            let slash = content_type[..plus].rfind('/')?;
            Some(format!("{}/{}", &content_type[..slash], &content_type[plus + 1..]))
        });
        for mime in std::iter::once(content_type.as_str()).chain(general.as_deref()) {
            if let Some(l) = crate::registry::lexer_for_mimetype(mime) {
                lex_sub(l.imp(), cx.text, s, e, None, cx.out);
                return;
            }
        }
    }
    cx.emit(TokenType::TEXT, s, e);
}

pub(super) fn notmuch_highlight_code(cx: &mut Cx<'_>, m: &Groups<'_>, _: &Value) {
    let Some((s, e)) = m.get(1) else { return };
    // `body_lexer` is unset by default, so the body language is guessed.
    let lexer = crate::registry::guess_lexer(py_strip(&cx.text[s..e])).map(|l| l.imp()).or_else(|| by_alias("text"));
    match lexer {
        Some(l) => lex_sub(l, cx.text, s, e, None, cx.out),
        None => cx.emit(TokenType::TEXT, s, e),
    }
}
