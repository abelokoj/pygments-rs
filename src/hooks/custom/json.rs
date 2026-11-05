//! `JsonLexer` and `JsonLdLexer` from `pygments.lexers.data`: one pass over the bytes.

use crate::engine::Tok;
use crate::lexer::LexerImpl;
use crate::token::TokenType;

#[derive(Clone, Copy)]
enum St {
    None,
    Str {
        escape: bool,
        unicode: u8,
    },
    Ws,
    Constant,
    Number {
        float: bool,
    },
    Punct,
    LineComment,
    BlockComment {
        star: bool,
    },
    /// Saw `/`, expecting `/` or `*`.
    Slash,
}

fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\n' | b'\r' | b'\t')
}

fn is_int(b: u8) -> bool {
    b == b'-' || b.is_ascii_digit()
}

fn is_punct(b: u8) -> bool {
    matches!(b, b'{' | b'}' | b'[' | b']' | b',')
}

/// Length of the UTF-8 char starting with byte `b`.
fn char_len(b: u8) -> usize {
    match b {
        0..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        _ => 4,
    }
}

fn flush(queue: &mut Vec<Tok>, out: &mut Vec<Tok>) {
    out.append(queue);
}

/// Queue the token if the queue is non-empty, else emit it.
fn queue_or_emit(queue: &mut Vec<Tok>, out: &mut Vec<Tok>, t: Tok) {
    if queue.is_empty() { out.push(t) } else { queue.push(t) }
}

pub(super) fn json(_lexer: &'static LexerImpl, text: &str, out: &mut Vec<Tok>) {
    let b = text.as_bytes();
    let n = b.len();
    let mut st = St::None;
    let mut start = 0;
    // Strings and comments whose type depends on whether a ':' follows.
    let mut queue: Vec<Tok> = Vec::new();
    let mut stop = 0;
    while stop < n {
        let c = b[stop];
        match st {
            St::Str { escape, unicode } => {
                st = if unicode > 0 {
                    if c.is_ascii_hexdigit() {
                        St::Str { escape: unicode > 1, unicode: unicode - 1 }
                    } else {
                        St::Str { escape: false, unicode: 0 }
                    }
                } else if escape {
                    if c == b'u' { St::Str { escape: true, unicode: 4 } } else { St::Str { escape: false, unicode: 0 } }
                } else if c == b'\\' {
                    St::Str { escape: true, unicode: 0 }
                } else if c == b'"' {
                    queue.push(Tok::new(TokenType::LITERAL_STRING_DOUBLE, start, stop + 1));
                    St::None
                } else {
                    st
                };
                stop += 1;
                continue;
            }
            St::Ws => {
                if is_ws(c) {
                    stop += 1;
                    continue;
                }
                queue_or_emit(&mut queue, out, Tok::new(TokenType::TEXT_WHITESPACE, start, stop));
            }
            St::Constant => {
                if matches!(c, b't' | b'r' | b'u' | b'e' | b'f' | b'a' | b'l' | b's' | b'n') {
                    stop += 1;
                    continue;
                }
                out.push(Tok::new(TokenType::KEYWORD_CONSTANT, start, stop));
            }
            St::Number { float } => {
                if is_int(c) {
                    stop += 1;
                    continue;
                }
                if matches!(c, b'.' | b'e' | b'E' | b'+') {
                    st = St::Number { float: true };
                    stop += 1;
                    continue;
                }
                let ty = if float { TokenType::LITERAL_NUMBER_FLOAT } else { TokenType::LITERAL_NUMBER_INTEGER };
                out.push(Tok::new(ty, start, stop));
            }
            St::Punct => {
                if is_punct(c) {
                    stop += 1;
                    continue;
                }
                out.push(Tok::new(TokenType::PUNCTUATION, start, stop));
            }
            St::LineComment => {
                if c != b'\n' {
                    stop += 1;
                    continue;
                }
                queue_or_emit(&mut queue, out, Tok::new(TokenType::COMMENT_SINGLE, start, stop));
            }
            St::BlockComment { star } => {
                if c == b'*' {
                    st = St::BlockComment { star: true };
                } else if star {
                    st = St::BlockComment { star: false };
                    if c == b'/' {
                        queue_or_emit(&mut queue, out, Tok::new(TokenType::COMMENT_MULTILINE, start, stop + 1));
                        st = St::None;
                    }
                }
                stop += 1;
                continue;
            }
            St::Slash => {
                if c == b'/' {
                    st = St::LineComment;
                    stop += 1;
                    continue;
                }
                if c == b'*' {
                    st = St::BlockComment { star: false };
                    stop += 1;
                    continue;
                }
                flush(&mut queue, out);
                out.push(Tok::new(TokenType::ERROR, start, stop));
            }
            St::None => {}
        }
        // Evaluate `c` as the start of a new token.
        start = stop;
        st = St::None;
        let mut len = 1;
        match c {
            b'"' => st = St::Str { escape: false, unicode: 0 },
            c if is_ws(c) => st = St::Ws,
            b'f' | b'n' | b't' => {
                flush(&mut queue, out);
                st = St::Constant;
            }
            c if is_int(c) => {
                flush(&mut queue, out);
                st = St::Number { float: false };
            }
            b':' => {
                for mut t in queue.drain(..) {
                    if t.ty == TokenType::LITERAL_STRING_DOUBLE {
                        t.ty = TokenType::NAME_TAG;
                    }
                    out.push(t);
                }
                st = St::Punct;
            }
            c if is_punct(c) => {
                flush(&mut queue, out);
                st = St::Punct;
            }
            b'/' => st = St::Slash,
            c => {
                flush(&mut queue, out);
                len = char_len(c).min(n - stop);
                out.push(Tok::new(TokenType::ERROR, start, start + len));
            }
        }
        stop += len;
    }
    flush(&mut queue, out);
    let tail = |ty| Tok::new(ty, start, n);
    match st {
        St::Str { .. } | St::BlockComment { .. } | St::Slash => out.push(tail(TokenType::ERROR)),
        St::Number { float: true } => out.push(tail(TokenType::LITERAL_NUMBER_FLOAT)),
        St::Number { float: false } => out.push(tail(TokenType::LITERAL_NUMBER_INTEGER)),
        St::Constant => out.push(tail(TokenType::KEYWORD_CONSTANT)),
        St::Ws => out.push(tail(TokenType::TEXT_WHITESPACE)),
        St::Punct => out.push(tail(TokenType::PUNCTUATION)),
        St::LineComment => out.push(tail(TokenType::COMMENT_SINGLE)),
        St::None => {}
    }
}

pub(super) fn json_ld(lexer: &'static LexerImpl, text: &str, out: &mut Vec<Tok>) {
    let first = out.len();
    json(lexer, text, out);
    let keywords = lexer.word_set("json_ld_keywords");
    for t in &mut out[first..] {
        if t.ty == TokenType::NAME_TAG && text.get(t.start..t.end).is_some_and(|v| keywords.contains(v)) {
            t.ty = TokenType::NAME_DECORATOR;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{exp, lex};

    #[test]
    fn json_object() {
        assert_eq!(
            lex("json", "{\"a\": [1, 2.5e3, true], \"b\" /*c*/ : null} // x\n"),
            exp(&[
                ("Punctuation", "{"),
                ("Name.Tag", "\"a\""),
                ("Punctuation", ":"),
                ("Text.Whitespace", " "),
                ("Punctuation", "["),
                ("Literal.Number.Integer", "1"),
                ("Punctuation", ","),
                ("Text.Whitespace", " "),
                ("Literal.Number.Float", "2.5e3"),
                ("Punctuation", ","),
                ("Text.Whitespace", " "),
                ("Keyword.Constant", "true"),
                ("Punctuation", "],"),
                ("Text.Whitespace", " "),
                ("Name.Tag", "\"b\""),
                ("Text.Whitespace", " "),
                ("Comment.Multiline", "/*c*/"),
                ("Text.Whitespace", " "),
                ("Punctuation", ":"),
                ("Text.Whitespace", " "),
                ("Keyword.Constant", "null"),
                ("Punctuation", "}"),
                ("Text.Whitespace", " "),
                ("Comment.Single", "// x"),
                ("Text.Whitespace", "\n"),
            ])
        );
    }

    #[test]
    fn json_errors() {
        assert_eq!(
            lex("json", "\"x\" é /a \"\\u12\"\n"),
            exp(&[
                ("Literal.String.Double", "\"x\""),
                ("Text.Whitespace", " "),
                ("Error", "é"),
                ("Text.Whitespace", " "),
                ("Error", "/"),
                ("Error", "a"),
                ("Text.Whitespace", " "),
                ("Error", "\"\\u12\"\n"),
            ])
        );
    }

    #[test]
    fn json_ld_keyword() {
        assert_eq!(
            lex("jsonld", "{\"@id\": \"x\"}\n"),
            exp(&[
                ("Punctuation", "{"),
                ("Name.Decorator", "\"@id\""),
                ("Punctuation", ":"),
                ("Text.Whitespace", " "),
                ("Literal.String.Double", "\"x\""),
                ("Punctuation", "}"),
                ("Text.Whitespace", "\n"),
            ])
        );
    }
}
