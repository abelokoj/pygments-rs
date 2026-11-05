//! Rule callbacks of the Pygments modules `data`, `markup`, `css`, `ruby`,
//! `crystal`, `perl`, `textfmts`, `mime`, `sql`, `configs` and `dsls`.

use serde_json::Value;

use super::{Cx, Groups, Module};
use crate::engine::{Run, StateId};
use crate::lexer::LexerImpl;
use crate::token::TokenType;

mod css;
mod data;
mod dsls;
mod markup;
mod mime;
mod perl;
mod ruby;
mod sql;
mod textfmts;

#[cfg(test)]
mod tests;

pub(crate) static MODULE: Module = Module {
    callbacks: &[
        ("configs::TerraformLexer.heredoc_callback", ruby::terraform_heredoc),
        ("crystal::CrystalLexer.heredoc_callback", ruby::crystal_heredoc),
        ("css::_indentation", css::indentation),
        ("css::_starts_block.<locals>.callback", css::starts_block),
        ("data::YamlLexer.parse_block_scalar_empty_line.<locals>.callback", data::parse_block_scalar_empty_line),
        ("data::YamlLexer.parse_block_scalar_indent.<locals>.callback", data::parse_block_scalar_indent),
        ("data::YamlLexer.parse_plain_scalar_indent.<locals>.callback", data::parse_plain_scalar_indent),
        ("data::YamlLexer.reset_indent.<locals>.callback", data::reset_indent),
        ("data::YamlLexer.save_indent.<locals>.callback", data::save_indent),
        ("data::YamlLexer.set_block_scalar_indent.<locals>.callback", data::set_block_scalar_indent),
        ("data::YamlLexer.set_indent.<locals>.callback", data::set_indent),
        ("data::YamlLexer.something.<locals>.callback", data::something),
        ("dsls::SnowballLexer._string.<locals>.callback", dsls::string),
        ("dsls::SnowballLexer._stringescapes", dsls::stringescapes),
        ("markup::MarkdownLexer._handle_codeblock", markup::markdown_codeblock),
        ("markup::RstLexer._handle_sourcecode", markup::rst_sourcecode),
        ("markup::TiddlyWiki5Lexer._handle_codeblock", markup::tiddly_codeblock),
        ("markup::TiddlyWiki5Lexer._handle_cssblock", markup::tiddly_cssblock),
        ("markup::WikitextLexer.handle_score", markup::wikitext_score),
        ("markup::WikitextLexer.handle_syntaxhighlight", markup::wikitext_syntaxhighlight),
        ("mime::MIMELexer.get_body_tokens", mime::get_body_tokens),
        ("mime::MIMELexer.get_content_type_subtokens", mime::get_content_type_subtokens),
        ("mime::MIMELexer.get_header_tokens", mime::get_header_tokens),
        ("mime::MIMELexer.store_content_transfer_encoding", mime::store_content_transfer_encoding),
        ("mime::MIMELexer.store_content_type", mime::store_content_type),
        ("perl::Perl6Lexer.brackets_callback.<locals>.callback", perl::brackets),
        ("perl::Perl6Lexer.closing_brace_callback", perl::closing_brace),
        ("perl::Perl6Lexer.embedded_perl6_callback", perl::embedded_perl6),
        ("perl::Perl6Lexer.opening_brace_callback", perl::opening_brace),
        ("ruby::RubyLexer.gen_rubystrings_rules.<locals>.intp_regex_callback", ruby::intp_regex),
        ("ruby::RubyLexer.gen_rubystrings_rules.<locals>.intp_string_callback", ruby::intp_string),
        ("ruby::RubyLexer.heredoc_callback", ruby::ruby_heredoc),
        ("sql::language_callback", sql::language_callback),
        ("textfmts::HttpLexer.content_callback", textfmts::http_content),
        ("textfmts::HttpLexer.continuous_header_callback", textfmts::http_continuous_header),
        ("textfmts::HttpLexer.header_callback", textfmts::http_header),
        ("textfmts::NotmuchLexer._highlight_code", textfmts::notmuch_highlight_code),
    ],
    filters: &[],
    custom: &[],
    analyse: &[],
};

// -- shared helpers -----------------------------------------------------------------

/// Token type from a converter arg such as `["Name", "Tag"]`, plus an optional
/// dotted `suffix` (`".Error"`). Builds the name on the stack: callbacks run per token.
fn arg_ty(args: &Value, key: &str, suffix: &str) -> TokenType {
    let mut buf = [0u8; 128];
    let mut n = 0;
    let mut push = |s: &str| {
        let b = s.as_bytes();
        if n + b.len() <= buf.len() {
            buf[n..n + b.len()].copy_from_slice(b);
        }
        n += b.len();
    };
    push("Token");
    for part in args.get(key).and_then(Value::as_array).into_iter().flatten() {
        push(".");
        push(part.as_str().unwrap_or(""));
    }
    push(suffix);
    buf.get(..n)
        .and_then(|b| std::str::from_utf8(b).ok())
        .and_then(TokenType::from_name)
        .unwrap_or(if suffix.is_empty() { TokenType::TOKEN } else { TokenType::ERROR })
}

fn arg_bool(args: &Value, key: &str) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// `yield match.start(i), ty, match.group(i)`; skipped when the group did not
/// participate (Python would yield `None`).
fn emit_g(cx: &mut Cx<'_>, m: &Groups<'_>, i: usize, ty: TokenType) {
    if let Some((s, e)) = m.get(i) {
        cx.emit(ty, s, e);
    }
}

/// Python's `str.isspace()` set for `strip()`.
fn py_ws(c: char) -> bool {
    c.is_whitespace() || ('\x1c'..='\x1f').contains(&c)
}

fn py_strip(s: &str) -> &str {
    s.trim_matches(py_ws)
}

fn py_rstrip(s: &str) -> &str {
    s.trim_end_matches(py_ws)
}

/// Byte offset `n` characters after `pos`, clamped to the text end.
fn fwd_chars(text: &str, pos: usize, n: usize) -> usize {
    let pos = pos.min(text.len());
    text[pos..].char_indices().nth(n).map_or(text.len(), |(i, _)| pos + i)
}

/// Byte offset `n` characters before `pos`, clamped to 0.
fn back_chars(text: &str, pos: usize, n: usize) -> usize {
    if n == 0 {
        return pos;
    }
    text[..pos].char_indices().rev().nth(n - 1).map_or(0, |(i, _)| i)
}

/// `str.find(needle, from)` with byte offsets.
fn find_from(text: &str, needle: &str, from: usize) -> Option<usize> {
    text.get(from..)?.find(needle).map(|i| from + i)
}

/// `re.escape` for building an Oniguruma pattern.
fn re_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || !c.is_ascii() {
            out.push(c);
        } else {
            out.push_str(&format!("\\x{:02x}", c as u32));
        }
    }
    out
}

/// `get_lexer_by_name(alias)` (default options).
fn by_alias(alias: &str) -> Option<&'static LexerImpl> {
    if alias.is_empty() { None } else { crate::registry::lexer_by_name(alias).map(|l| l.imp()) }
}

/// Run this same lexer over `cx.text[start..end]` as its own text, sharing
/// `cx.extra` (Pygments calls `self.get_tokens_unprocessed` on the same instance).
fn lex_shared(cx: &mut Cx<'_>, start: usize, end: usize, names: &[&str]) {
    let Some(sub) = cx.text.get(start..end) else { return };
    let stack: Vec<StateId> = cx.lexer.regex().and_then(|r| r.stack_from_names(names)).unwrap_or_default();
    let extra = std::mem::take(cx.extra);
    let mut run = Run::new(cx.lexer, sub, 0, stack, extra);
    let first = cx.out.len();
    while run.step() {
        cx.out.extend(run.out.drain(..));
    }
    cx.out.extend(run.out.drain(..));
    *cx.extra = run.extra;
    for t in &mut cx.out[first..] {
        t.start += start;
        t.end += start;
    }
}
