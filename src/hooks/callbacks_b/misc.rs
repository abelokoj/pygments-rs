//! Callbacks from small Pygments modules: arturo, asm, bibtex, csound,
//! fortran, haxe, lisp, maple, ml and urbi.

use std::collections::HashSet;

use serde_json::Value;

use crate::engine::{Run, StateId};
use crate::hooks::{Cx, Groups, lex_sub};
use crate::token::TokenType;

/// Emit group `i` as `ty` if it participated (empty groups too, as Python does).
fn group(cx: &mut Cx<'_>, g: &Groups<'_>, i: usize, ty: TokenType) {
    if let Some((s, e)) = g.get(i) {
        cx.emit(ty, s, e);
    }
}

// -- arturo -------------------------------------------------------------------

/// `ArturoLexer.handle_annotated_strings`: `{!lang code}` lexed with `lang`.
pub(super) fn arturo_annotated(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    group(cx, g, 1, TokenType::LITERAL_STRING_DOUBLE);
    group(cx, g, 2, TokenType::LITERAL_STRING_INTERPOL);
    group(cx, g, 3, TokenType::LITERAL_STRING_INTERPOL);
    group(cx, g, 4, TokenType::TEXT_WHITESPACE);
    let name = g.str(cx.text, 3).unwrap_or("").trim();
    if let Some((s, e)) = g.get(5) {
        match crate::lexer_by_name(name) {
            Some(l) => lex_sub(l.imp(), cx.text, s, e, None, cx.out),
            None => cx.emit(TokenType::LITERAL_STRING, s, e),
        }
    }
    group(cx, g, 6, TokenType::LITERAL_STRING_DOUBLE);
}

// -- asm ------------------------------------------------------------------------

/// `Dasm16Lexer.guess_identifier`: registers vs labels.
pub(super) fn dasm16_identifier(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    let ident = g.str(cx.text, 0).unwrap_or("");
    let register = cx.lexer.word_set("REGISTERS").iter().any(|r| r.eq_ignore_ascii_case(ident));
    let ty = if register { TokenType::NAME_VARIABLE } else { TokenType::NAME_LABEL };
    cx.emit(ty, g.start(), g.end());
}

// -- bibtex ---------------------------------------------------------------------

/// `ctx.opening_brace` of the BibTeX lexer.
#[derive(Clone, Debug, Default, PartialEq)]
struct BibBrace(Option<u8>);

pub(super) fn bibtex_open(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    cx.extra.get::<BibBrace>().0 = cx.text.as_bytes().get(g.start()).copied();
    cx.emit(TokenType::PUNCTUATION, g.start(), g.end());
    cx.pos = g.end();
}

pub(super) fn bibtex_close(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    let close = cx.text.as_bytes().get(g.start()).copied();
    let open = cx.extra.get::<BibBrace>().0.take();
    let bad = (open == Some(b'{') && close != Some(b'}')) || (open == Some(b'(') && close != Some(b')'));
    cx.emit(if bad { TokenType::ERROR } else { TokenType::PUNCTUATION }, g.start(), g.end());
    cx.pos = g.end();
}

// -- csound ---------------------------------------------------------------------

/// `CsoundOrchestraLexer.user_defined_opcodes` (a class attribute in Python;
/// kept per tokenization here).
#[derive(Clone, Debug, Default, PartialEq)]
struct CsoundOpcodes(HashSet<Box<str>>);

pub(super) fn csound_opcode_name(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    let opcode = g.str(cx.text, 0).unwrap_or("");
    cx.extra.get::<CsoundOpcodes>().0.insert(opcode.into());
    cx.emit(TokenType::NAME_FUNCTION, g.start(), g.end());
}

pub(super) fn csound_name(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    let Some((s, e)) = g.get(1) else { return };
    let text = cx.text;
    let name = &text[s..e];
    let l = cx.lexer;
    let mut annotation = TokenType::KEYWORD_TYPE;
    if l.word_set("OPCODES").contains(name)
        || l.word_set("DEPRECATED_OPCODES").contains(name)
        || l.word_set("REMOVED_OPCODES").contains(name)
    {
        cx.emit(TokenType::NAME_BUILTIN, s, e);
    } else if cx.extra.get::<CsoundOpcodes>().0.contains(name) {
        cx.emit(TokenType::NAME_FUNCTION, s, e);
    } else {
        annotation = TokenType::NAME;
        // re.search(r'^(g?[afikSw])(\w+)', name); `name` is all word characters.
        let b = name.as_bytes();
        let kind = |i: usize| b.get(i).is_some_and(|c| b"afikSw".contains(c));
        let split = if b.first() == Some(&b'g') && kind(1) && b.len() > 2 {
            2
        } else if kind(0) && b.len() > 1 {
            1
        } else {
            0
        };
        if split > 0 {
            cx.emit(TokenType::KEYWORD_TYPE, s, s + split);
            cx.emit(TokenType::NAME, s + split, e);
        } else {
            cx.emit(TokenType::NAME, s, e);
        }
    }
    if let Some((s2, e2)) = g.get(2).filter(|(s2, e2)| e2 > s2) {
        cx.emit(TokenType::PUNCTUATION, s2, e2);
        group(cx, g, 3, annotation);
    }
}

// -- fortran --------------------------------------------------------------------

/// `FortranFixedLexer._lex_fortran`: lex the line (plus a newline) as free-form
/// Fortran and drop the newline again.
pub(super) fn fortran_fixed_line(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    let (s, e) = (g.start(), g.end());
    let Some(fortran) = crate::registry::by_class("FortranLexer") else {
        cx.emit(TokenType::TOKEN, s, e);
        return;
    };
    let mut line = String::with_capacity(e - s + 1);
    line.push_str(&cx.text[s..e]);
    line.push('\n');
    let mut toks = Vec::new();
    fortran.tokens_unprocessed(&line, None, &mut toks);
    // The appended newline is the only one in `line`; clip it off.
    let len = e - s;
    for t in toks {
        let end = t.end.min(len);
        if end > t.start {
            cx.emit(t.ty, s + t.start, s + end);
        }
    }
}

// -- haxe -----------------------------------------------------------------------

/// `HaxeLexer.preproc_stack` (a class attribute in Python; kept per
/// tokenization here).
#[derive(Clone, Debug, Default, PartialEq)]
struct HaxePreproc(Vec<Vec<StateId>>);

pub(super) fn haxe_preproc(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    let proc_ = g.str(cx.text, 2).unwrap_or("");
    match proc_ {
        "if" => {
            let saved = cx.stack.clone();
            cx.extra.get::<HaxePreproc>().0.push(saved);
        }
        "else" | "elseif" => {
            if let Some(saved) = cx.extra.get::<HaxePreproc>().0.last().cloned() {
                *cx.stack = saved;
            }
        }
        "end" => {
            cx.extra.get::<HaxePreproc>().0.pop();
        }
        _ => {}
    }
    match proc_ {
        "if" | "elseif" => cx.push_state("preproc-expr"),
        "error" => cx.push_state("preproc-error"),
        _ => {}
    }
    cx.emit(TokenType::COMMENT_PREPROC, g.start(), g.end());
    cx.pos = g.end();
}

// -- lisp -----------------------------------------------------------------------

/// `SchemeLexer.decimal_cb`.
pub(super) fn scheme_decimal(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    let float = g.str(cx.text, 0).is_some_and(|s| s.contains('.'));
    let ty = if float { TokenType::LITERAL_NUMBER_FLOAT } else { TokenType::LITERAL_NUMBER_INTEGER };
    cx.emit(ty, g.start(), g.end());
}

// -- maple ----------------------------------------------------------------------

/// `MapleLexer.delayed_callback`: the quoted expression is lexed in place with
/// the current stack, the text cut off at the closing quotes.
pub(super) fn maple_delayed(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    let (Some((q0, q1)), Some((s2, e2))) = (g.get(1), g.get(2)) else { return };
    cx.emit(TokenType::PUNCTUATION, q0, q1);
    let text = cx.text;
    let mut run = Run::new(cx.lexer, &text[..e2], s2, std::mem::take(cx.stack), std::mem::take(cx.extra));
    while run.step() {
        cx.out.extend(run.out.drain(..));
    }
    cx.out.extend(run.out.drain(..));
    *cx.stack = run.stack;
    *cx.extra = run.extra;
    cx.emit(TokenType::PUNCTUATION, e2, e2 + (q1 - q0));
    cx.pos = g.end();
}

// -- ml -------------------------------------------------------------------------

pub(super) fn sml_long_id(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    let reserved = g.str(cx.text, 1).is_some_and(|s| cx.lexer.word_set("alphanumid_reserved").contains(s));
    group(cx, g, 1, if reserved { TokenType::ERROR } else { TokenType::NAME_NAMESPACE });
    group(cx, g, 2, TokenType::PUNCTUATION);
}

pub(super) fn sml_end_id(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    let l = cx.lexer;
    let reserved = g.str(cx.text, 1).is_some_and(|s| {
        l.word_set("alphanumid_reserved").contains(s) || l.word_set("symbolicid_reserved").contains(s)
    });
    group(cx, g, 1, if reserved { TokenType::ERROR } else { TokenType::NAME });
}

pub(super) fn sml_id(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    let l = cx.lexer;
    let s = g.str(cx.text, 1).unwrap_or("");
    let ty = if l.word_set("alphanumid_reserved").contains(s) {
        TokenType::KEYWORD_RESERVED
    } else if l.word_set("symbolicid_reserved").contains(s) {
        TokenType::PUNCTUATION
    } else {
        TokenType::NAME
    };
    group(cx, g, 1, ty);
}

// -- urbi -----------------------------------------------------------------------

/// `UrbiscriptLexer.blob_callback`: `\B(n)(` followed by `n` characters and `)`.
pub(super) fn urbi_blob(cx: &mut Cx<'_>, g: &Groups<'_>, _args: &Value) {
    let (Some((b0, b1)), Some((s2, e2))) = (g.get(1), g.get(2)) else { return };
    cx.emit(TokenType::LITERAL_STRING, b0, b1);
    cx.pos += b1 - b0;
    let end = g.end();
    // Python indexes `ctx.text[end + size]` by characters; out of range or an
    // unparsable size raises there, which we treat as a malformed blob.
    let close = g
        .str(cx.text, 3)
        .and_then(|n| n.parse::<usize>().ok())
        .and_then(|n| cx.text[end..].char_indices().nth(n))
        .filter(|&(_, c)| c == ')')
        .map(|(i, _)| end + i + 1);
    match close {
        Some(c) => {
            cx.emit(TokenType::LITERAL_STRING_ESCAPE, s2, c);
            cx.pos = c;
        }
        None => {
            cx.emit(TokenType::LITERAL_STRING, s2, e2);
            cx.pos = e2;
        }
    }
}
