//! `pygments.lexers.sql.language_callback` (Postgres `$$` strings).

use std::sync::LazyLock;

use onig::{Regex, Region, SearchOptions};
use serde_json::Value;

use super::{back_chars, by_alias, emit_g, fwd_chars};
use crate::engine::compile;
use crate::hooks::{Cx, Groups, lex_sub};
use crate::lexer::LexerImpl;
use crate::token::TokenType;

static LANGUAGE_RE: LazyLock<Regex> = LazyLock::new(|| compile(r"\s+LANGUAGE\s+'?(\w+)'?", "i").expect("valid regex"));
static DO_RE: LazyLock<Regex> = LazyLock::new(|| compile(r"\bDO\b", "i").expect("valid regex"));

/// `PostgresBase._get_lexer`.
fn get_lexer(lang: &str) -> Option<&'static LexerImpl> {
    if lang.eq_ignore_ascii_case("sql") {
        return by_alias("postgresql");
    }
    let pl = lang.strip_prefix("pl");
    let u = lang.strip_suffix('u');
    let both = pl.and_then(|r| r.strip_suffix('u'));
    [Some(lang), pl, u, both].into_iter().flatten().find_map(by_alias)
}

pub(super) fn language_callback(cx: &mut Cx<'_>, m: &Groups<'_>, _: &Value) {
    let text = cx.text;
    let (s, e) = (m.start(), m.end());
    let after = &text[e..fwd_chars(text, e, 100)];
    let mut region = Region::new();
    let lx = if LANGUAGE_RE.match_with_options(after, 0, SearchOptions::SEARCH_OPTION_NONE, Some(&mut region)).is_some()
    {
        region.pos(1).and_then(|(a, b)| get_lexer(&after[a..b]))
    } else {
        let before = &text[back_chars(text, s, 100)..s];
        match LANGUAGE_RE.captures_iter(before).last() {
            Some(c) => c.at(1).and_then(get_lexer),
            None if DO_RE.find(&text[back_chars(text, s, 25)..s]).is_some() => get_lexer("plpgsql"),
            None => None,
        }
    };
    emit_g(cx, m, 1, TokenType::LITERAL_STRING);
    emit_g(cx, m, 2, TokenType::LITERAL_STRING_DELIMITER);
    emit_g(cx, m, 3, TokenType::LITERAL_STRING);
    match (lx, m.get(4)) {
        (Some(l), Some((a, b))) => lex_sub(l, text, a, b, None, cx.out),
        _ => emit_g(cx, m, 4, TokenType::LITERAL_STRING),
    }
    emit_g(cx, m, 5, TokenType::LITERAL_STRING);
    emit_g(cx, m, 6, TokenType::LITERAL_STRING_DELIMITER);
    emit_g(cx, m, 7, TokenType::LITERAL_STRING);
}
