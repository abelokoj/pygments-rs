//! `ErbLexer` (`pygments.lexers.templates`) and `GosuTemplateLexer`
//! (`pygments.lexers.jvm`).

use std::sync::LazyLock;

use onig::Regex;

use crate::engine::{Tok, compile};
use crate::hooks::lex_sub;
use crate::lexer::LexerImpl;
use crate::token::TokenType;

static BLOCK_RE: LazyLock<Regex> =
    LazyLock::new(|| compile(r"(<%%|%%>|<%=|<%#|<%-|<%|-%>|%>|^%[^%].*?$)", "m").expect("static ERB pattern"));

pub(super) fn erb(_lexer: &'static LexerImpl, text: &str, out: &mut Vec<Tok>) {
    // `_block_re.split(text)`: text, tag, text, tag, ..., text.
    let mut pieces = Vec::new();
    let mut last = 0;
    for (s, e) in BLOCK_RE.find_iter(text) {
        pieces.push((last, s));
        pieces.push((s, e));
        last = e;
    }
    pieces.push((last, text.len()));
    let ruby = crate::registry::by_class("RubyLexer");
    let ruby_lex = |s: usize, e: usize, out: &mut Vec<Tok>| match ruby {
        Some(r) => lex_sub(r, text, s, e, None, out),
        None => out.push(Tok::new(TokenType::TEXT, s, e)),
    };
    let mut it = pieces.into_iter();
    let mut state = 0;
    // Python ends on the first `pop()` from the exhausted list.
    while let Some((s, e)) = it.next() {
        let tag = &text[s..e];
        match state {
            0 => {
                out.push(Tok::new(TokenType::OTHER, s, e));
                state = 1;
            }
            1 => match tag {
                "<%%" | "%%>" => {
                    out.push(Tok::new(TokenType::OTHER, s, e));
                    state = 0;
                }
                "<%#" => {
                    out.push(Tok::new(TokenType::COMMENT_PREPROC, s, e));
                    let Some((vs, ve)) = it.next() else { return };
                    out.push(Tok::new(TokenType::COMMENT, vs, ve));
                    state = 2;
                }
                "<%" | "<%=" | "<%-" => {
                    out.push(Tok::new(TokenType::COMMENT_PREPROC, s, e));
                    let Some((ds, de)) = it.next() else { return };
                    ruby_lex(ds, de, out);
                    state = 2;
                }
                "%>" | "-%>" => {
                    out.push(Tok::new(TokenType::ERROR, s, e));
                    state = 0;
                }
                _ => {
                    // `%` line of raw Ruby.
                    out.push(Tok::new(TokenType::COMMENT_PREPROC, s, s + 1));
                    ruby_lex(s + 1, e, out);
                    state = 0;
                }
            },
            _ => {
                let ty = if matches!(tag, "%>" | "-%>") { TokenType::COMMENT_PREPROC } else { TokenType::OTHER };
                out.push(Tok::new(ty, s, e));
                state = 0;
            }
        }
    }
}

pub(super) fn gosu_template(_lexer: &'static LexerImpl, text: &str, out: &mut Vec<Tok>) {
    match crate::registry::by_class("GosuLexer") {
        Some(g) => g.tokens_unprocessed(text, Some(&["templateText"][..]), out),
        None => out.push(Tok::new(TokenType::TEXT, 0, text.len())),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{exp, lex};

    #[test]
    fn erb_blocks() {
        assert_eq!(
            lex("erb", "a <%# c %>b<%= x %>\n% y\n<%% -%>\n"),
            exp(&[
                ("Other", "a "),
                ("Comment.Preproc", "<%#"),
                ("Comment", " c "),
                ("Comment.Preproc", "%>"),
                ("Other", "b"),
                ("Comment.Preproc", "<%="),
                ("Text.Whitespace", " "),
                ("Name", "x"),
                ("Text.Whitespace", " "),
                ("Comment.Preproc", "%>"),
                ("Other", "\n"),
                ("Comment.Preproc", "%"),
                ("Text.Whitespace", " "),
                ("Name", "y"),
                ("Other", "\n"),
                ("Other", "<%%"),
                ("Other", " "),
                ("Error", "-%>"),
                ("Other", "\n"),
            ])
        );
    }

    #[test]
    fn gosu_template_text() {
        assert_eq!(
            lex("gst", "a <%= b %>\n"),
            exp(&[
                ("Literal.String", "a"),
                ("Literal.String", " "),
                ("Operator", "<%"),
                ("Operator", "="),
                ("Text.Whitespace", " "),
                ("Name", "b"),
                ("Text.Whitespace", " "),
                ("Operator", "%>"),
                ("Literal.String", "\n"),
            ])
        );
    }
}
