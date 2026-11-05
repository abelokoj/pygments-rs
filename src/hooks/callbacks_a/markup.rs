//! `pygments.lexers.markup` callbacks: embedded code blocks of Markdown,
//! reStructuredText, TiddlyWiki5 and Wikitext.

use std::sync::LazyLock;

use onig::Regex;
use serde_json::Value;

use super::{by_alias, emit_g, py_strip};
use crate::engine::{Tok, compile};
use crate::hooks::{Cx, Groups, do_insertions, lex_sub};
use crate::lexer::LexerImpl;
use crate::token::TokenType;

/// The `handlecodeblocks` option (default true) and the named lexer.
fn code_lexer(this: &LexerImpl, alias: &str) -> Option<&'static LexerImpl> {
    if this.konst("handlecodeblocks").and_then(Value::as_bool).unwrap_or(true) { by_alias(alias) } else { None }
}

/// Lex group `i` with `lexer`, or emit it as `fallback`.
fn code_group(cx: &mut Cx<'_>, m: &Groups<'_>, i: usize, lexer: &'static LexerImpl) {
    if let Some((s, e)) = m.get(i) {
        lex_sub(lexer, cx.text, s, e, None, cx.out);
    }
}

pub(super) fn markdown_codeblock(cx: &mut Cx<'_>, m: &Groups<'_>, _: &Value) {
    // Groups: 1 initial, 2 lang, 3 afterlang, 4 whitespace, 5 extra, 6 newline,
    // 7 code, 9 terminator.
    emit_g(cx, m, 1, TokenType::LITERAL_STRING_BACKTICK);
    emit_g(cx, m, 2, TokenType::LITERAL_STRING_BACKTICK);
    if m.get(3).is_some() {
        emit_g(cx, m, 4, TokenType::TEXT_WHITESPACE);
        emit_g(cx, m, 5, TokenType::TEXT);
    }
    emit_g(cx, m, 6, TokenType::TEXT_WHITESPACE);
    let this = cx.lexer;
    let lexer = m.str(cx.text, 2).and_then(|l| code_lexer(this, py_strip(l)));
    match lexer {
        Some(l) => code_group(cx, m, 7, l),
        None => emit_g(cx, m, 7, TokenType::LITERAL_STRING),
    }
    emit_g(cx, m, 9, TokenType::LITERAL_STRING_BACKTICK);
}

/// Python's `str.splitlines(keepends=True)`.
fn splitlines(s: &str) -> impl Iterator<Item = &str> {
    let mut rest = s;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let mut end = rest.len();
        for (i, c) in rest.char_indices() {
            if matches!(
                c,
                '\n' | '\r' | '\x0b' | '\x0c' | '\x1c' | '\x1d' | '\x1e' | '\u{85}' | '\u{2028}' | '\u{2029}'
            ) {
                end = i + c.len_utf8();
                if c == '\r' && rest[end..].starts_with('\n') {
                    end += 1;
                }
                break;
            }
        }
        let (line, tail) = rest.split_at(end);
        rest = tail;
        Some(line)
    })
}

pub(super) fn rst_sourcecode(cx: &mut Cx<'_>, m: &Groups<'_>, _: &Value) {
    let text = cx.text;
    let header = [
        TokenType::PUNCTUATION,
        TokenType::TEXT,
        TokenType::OPERATOR_WORD,
        TokenType::PUNCTUATION,
        TokenType::TEXT,
        TokenType::KEYWORD,
        TokenType::TEXT,
    ];
    for (i, ty) in header.into_iter().enumerate() {
        emit_g(cx, m, i + 1, ty);
    }
    let (Some((s8, e8)), Some((_, e11))) = (m.get(8), m.get(11)) else { return };
    let this = cx.lexer;
    let lexer = m.str(text, 6).and_then(|l| code_lexer(this, py_strip(l)));
    let Some(lexer) = lexer else {
        cx.emit(TokenType::LITERAL_STRING, s8, e11);
        return;
    };
    let indent = text[s8..e8].chars().count();
    let mut code = String::with_capacity(e11 - s8);
    let mut ins = Vec::new();
    for line in splitlines(&text[s8..e11]) {
        match line.char_indices().nth(indent) {
            Some((cut, _)) => {
                ins.push((code.len(), vec![Tok::new(TokenType::TEXT, 0, cut)]));
                code.push_str(&line[cut..]);
            }
            None => code.push_str(line),
        }
    }
    let mut toks = Vec::new();
    lexer.tokens_unprocessed(&code, None, &mut toks);
    cx.out.extend(do_insertions(ins, toks).into_iter().map(|t| Tok::new(t.ty, t.start + s8, t.end + s8)));
}

pub(super) fn tiddly_codeblock(cx: &mut Cx<'_>, m: &Groups<'_>, _: &Value) {
    emit_g(cx, m, 1, TokenType::LITERAL_STRING);
    emit_g(cx, m, 2, TokenType::LITERAL_STRING);
    emit_g(cx, m, 3, TokenType::TEXT);
    let this = cx.lexer;
    let lexer = m.str(cx.text, 2).and_then(|l| code_lexer(this, py_strip(l)));
    let Some(lexer) = lexer else {
        // Pygments returns here, dropping the closing fence (group 5).
        emit_g(cx, m, 4, TokenType::LITERAL_STRING);
        return;
    };
    code_group(cx, m, 4, lexer);
    emit_g(cx, m, 5, TokenType::LITERAL_STRING);
}

pub(super) fn tiddly_cssblock(cx: &mut Cx<'_>, m: &Groups<'_>, _: &Value) {
    emit_g(cx, m, 1, TokenType::LITERAL_STRING);
    emit_g(cx, m, 2, TokenType::LITERAL_STRING);
    let Some(lexer) = code_lexer(cx.lexer, "css") else {
        // Pygments drops the closing tag (group 4) here too.
        emit_g(cx, m, 3, TokenType::LITERAL_STRING);
        return;
    };
    code_group(cx, m, 3, lexer);
    emit_g(cx, m, 4, TokenType::LITERAL_STRING);
}

static LANG_RE: LazyLock<Regex> = LazyLock::new(|| compile(r#"\blang=("|'|)(\w+)(\1)"#, "").expect("valid regex"));

pub(super) fn wikitext_syntaxhighlight(cx: &mut Cx<'_>, m: &Groups<'_>, _: &Value) {
    wikitext_tag(cx, m, false);
}

pub(super) fn wikitext_score(cx: &mut Cx<'_>, m: &Groups<'_>, _: &Value) {
    wikitext_tag(cx, m, true);
}

/// Called through `bygroups` on the tag's attributes and content.
fn wikitext_tag(cx: &mut Cx<'_>, m: &Groups<'_>, score: bool) {
    let text = cx.text;
    let (s, e) = (m.start(), m.end());
    let ac = &text[s..e];
    // The first `>` that does not end a comment `-->`.
    let mut from = 0;
    let index = loop {
        match ac[from..].find('>') {
            None => break None,
            Some(i) => {
                let i = from + i;
                if ac[..i].ends_with("--") {
                    from = i + 1;
                } else {
                    break Some(i);
                }
            }
        }
    };
    let Some(index) = index else {
        cx.lex_this(s, e, Some(&["root", "attr"]));
        return;
    };
    cx.lex_this(s, s + index, Some(&["root", "attr"]));
    cx.emit(TokenType::PUNCTUATION, s + index, s + index + 1);
    let content = s + index + 1;
    let lang = LANG_RE.captures_iter(&ac[..index]).last().and_then(|c| c.at(2));
    if score {
        if lang.unwrap_or("lilypond") == "lilypond" {
            cx.lex_with("LilyPondLexer", content, e, None);
        } else {
            cx.emit(TokenType::TEXT, content, e);
        }
        return;
    }
    match lang.and_then(by_alias) {
        Some(l) => lex_sub(l, text, content, e, None, cx.out),
        None => cx.emit(TokenType::TEXT, content, e),
    }
}
