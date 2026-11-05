//! `MakefileLexer` from `pygments.lexers.make`: preprocessor and comment lines
//! are cut out, the rest goes to `BaseMakefileLexer`, then spliced back.

use std::sync::LazyLock;

use onig::Regex;

use super::{match_at, py_lines, py_strip};
use crate::engine::{Tok, compile};
use crate::hooks::do_insertions;
use crate::lexer::LexerImpl;
use crate::token::TokenType;

static R_SPECIAL: LazyLock<Regex> = LazyLock::new(|| {
    compile(
        r"^(?:\.\s*(include|undef|error|warning|if|else|elif|endif|for|endfor)|\s*(ifeq|ifneq|ifdef|ifndef|else|endif|-?include|define|endef|:|vpath)|\s*(if|else|endif))(?=\s)",
        "",
    )
    .expect("static Makefile pattern")
});
static R_COMMENT: LazyLock<Regex> = LazyLock::new(|| compile(r"^\s*@?#", "").expect("static Makefile pattern"));

pub(super) fn makefile(_lexer: &'static LexerImpl, text: &str, out: &mut Vec<Tok>) {
    let Some(base) = crate::registry::by_class("BaseMakefileLexer") else {
        out.push(Tok::new(TokenType::TEXT, 0, text.len()));
        return;
    };
    let mut ins = Vec::new();
    let mut done = String::new();
    let mut backslash = false;
    for (s, _, e) in py_lines(text) {
        let line = &text[s..e];
        if match_at(&R_SPECIAL, line, 0).is_some() || backslash {
            ins.push((done.len(), vec![Tok::new(TokenType::COMMENT_PREPROC, 0, line.len())]));
            backslash = py_strip(line).ends_with('\\');
        } else if match_at(&R_COMMENT, line, 0).is_some() {
            ins.push((done.len(), vec![Tok::new(TokenType::COMMENT, 0, line.len())]));
        } else {
            done.push_str(line);
        }
    }
    let mut toks = Vec::new();
    base.tokens_unprocessed(&done, None, &mut toks);
    out.extend(do_insertions(ins, toks));
}

#[cfg(test)]
mod tests {
    use super::super::{exp, lex};

    #[test]
    fn makefile_insertions() {
        assert_eq!(
            lex("make", "# c\nifdef X \\\n  y\nendif\nall: a\n\techo $@\n"),
            exp(&[
                ("Comment", "# c\n"),
                ("Comment.Preproc", "ifdef X \\\n"),
                ("Comment.Preproc", "  y\n"),
                ("Comment.Preproc", "endif\n"),
                ("Name.Function", "all"),
                ("Operator", ":"),
                ("Text.Whitespace", " "),
                ("Name", "a"),
                ("Text.Whitespace", "\n"),
                ("Text.Whitespace", "\t"),
                ("Name.Builtin", "echo"),
                ("Text.Whitespace", " "),
                ("Name.Variable", "$@"),
                ("Text.Whitespace", "\n"),
            ])
        );
    }
}
