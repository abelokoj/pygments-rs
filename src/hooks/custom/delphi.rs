//! `DelphiLexer` and `PortugolLexer` from `pygments.lexers.pascal`: a
//! `pygments.scanner.Scanner` loop with keyword sets from the bundled consts.

use std::collections::HashSet;
use std::sync::LazyLock;

use onig::Regex;

use super::{match_at, py_strip};
use crate::engine::{Tok, compile};
use crate::lexer::LexerImpl;
use crate::token::TokenType;

struct Re {
    ws: Regex,
    comment: Regex,
    single: Regex,
    port_op: Regex,
    op: Regex,
    punct: Regex,
    ident: Regex,
    dot_test: Regex,
    dq: Regex,
    sq: Regex,
    chr: Regex,
    hex: Regex,
    int: Regex,
    float: Regex,
    esc: Regex,
    not_dq: Regex,
    not_sq: Regex,
    end: Regex,
    asm_label: Regex,
    asm_op: Regex,
    any: Regex,
}

static RE: LazyLock<Re> = LazyLock::new(|| {
    // Scanner(text, re.DOTALL | re.MULTILINE | re.IGNORECASE)
    let c = |p: &str| compile(p, "ims").expect("static Delphi pattern");
    Re {
        ws: c(r"\s+"),
        comment: c(r"\{.*?\}|\(\*.*?\*\)"),
        single: c(r"//.*?$"),
        port_op: c(r"(<\-)|(>=)|(<=)|%|<|>|-|\+|\*|\=|(<>)|\/|\.|:|,"),
        op: c(r"[-+*\/=<>:;,.@\^]"),
        punct: c(r"[\(\)\[\]]+"),
        ident: c(r"[A-Za-z_][A-Za-z_0-9]*"),
        dot_test: c(r"\s*\.\s*"),
        dq: c(r#"\""#),
        sq: c(r"'"),
        chr: c(r"\#(\d+|\$[0-9A-Fa-f]+)"),
        hex: c(r"\$[0-9A-Fa-f]+"),
        int: c(r"\d+(?![eE]|\.[^.])"),
        float: c(r"\d+(\.\d+([eE][+-]?\d+)?|[eE][+-]?\d+)"),
        esc: c(r"''"),
        not_dq: c(r#"[^\"]*"#),
        not_sq: c(r"[^']*"),
        end: c(r"end"),
        asm_label: c(r"@@[A-Za-z_][A-Za-z_0-9]*"),
        asm_op: c(r"[-+*\/=<>:;,.@\^]+"),
        any: c(r"."),
    }
});

/// `pygments.scanner.Scanner`.
struct Scanner<'t> {
    text: &'t str,
    pos: usize,
    start: usize,
    end: usize,
}

impl<'t> Scanner<'t> {
    fn scan(&mut self, re: &Regex) -> bool {
        match match_at(re, self.text, self.pos) {
            Some(n) => {
                self.start = self.pos;
                self.pos += n;
                self.end = self.pos;
                true
            }
            None => false,
        }
    }

    /// Python raises `EndOfText` at the end; that never happens on
    /// newline-terminated input, so treat it as no match.
    fn test(&self, re: &Regex) -> bool {
        self.pos < self.text.len() && match_at(re, self.text, self.pos).is_some()
    }

    fn get_char(&mut self) {
        self.scan(&RE.any);
    }

    fn matched(&self) -> &'t str {
        &self.text[self.start..self.end]
    }
}

#[derive(Clone, Copy, PartialEq)]
enum St {
    Initial,
    Str,
    Asm,
}

struct Sets<'a> {
    keywords: &'a HashSet<Box<str>>,
    builtins: &'a HashSet<Box<str>>,
    block_keywords: &'a HashSet<Box<str>>,
    function_modifiers: &'a HashSet<Box<str>>,
    builtin_types: &'a HashSet<Box<str>>,
    directives: &'a HashSet<Box<str>>,
    asm_instructions: &'a HashSet<Box<str>>,
    asm_registers: &'a HashSet<Box<str>>,
    portugol_builtin_types: &'a HashSet<Box<str>>,
}

pub(super) fn delphi(lexer: &'static LexerImpl, text: &str, out: &mut Vec<Tok>) {
    run(lexer, false, text, out);
}

/// `PortugolLexer` wraps `DelphiLexer(portugol=True)`; the word lists live
/// in `DelphiLexer`'s consts.
pub(super) fn portugol(_lexer: &'static LexerImpl, text: &str, out: &mut Vec<Tok>) {
    match crate::registry::by_class("DelphiLexer") {
        Some(d) => run(d, true, text, out),
        None => out.push(Tok::new(TokenType::TEXT, 0, text.len())),
    }
}

fn run(lx: &'static LexerImpl, portugol: bool, text: &str, out: &mut Vec<Tok>) {
    let w = |n: &'static str| lx.word_set(n);
    let sets = Sets {
        keywords: if portugol { w("PORTUGOL_KEYWORDS") } else { w("keywords") },
        builtins: if portugol { w("PORTUGOL_BUILTIN_TYPES") } else { w("builtins") },
        block_keywords: w("BLOCK_KEYWORDS"),
        function_modifiers: w("FUNCTION_MODIFIERS"),
        builtin_types: w("BUILTIN_TYPES"),
        directives: w("DIRECTIVES"),
        asm_instructions: w("ASM_INSTRUCTIONS"),
        asm_registers: w("ASM_REGISTERS"),
        portugol_builtin_types: w("PORTUGOL_BUILTIN_TYPES"),
    };
    let re = &*RE;
    let mut sc = Scanner { text, pos: 0, start: 0, end: 0 };
    let mut stack = vec![St::Initial];
    let mut in_function_block = false;
    let mut in_property_block = false;
    let mut was_dot = false;
    let mut next_token_is_function = false;
    let mut next_token_is_property = false;
    let mut collect_labels = false;
    let mut block_labels: HashSet<String> = HashSet::new();
    let mut brace_balance = [0i32; 2];
    let mut lower = String::new();

    while sc.pos < text.len() {
        let mut token = TokenType::ERROR;
        let top = *stack.last().unwrap_or(&St::Initial);
        match top {
            St::Initial => {
                if sc.scan(&re.ws) {
                    token = TokenType::TEXT_WHITESPACE;
                } else if !portugol && sc.scan(&re.comment) {
                    token = if sc.matched().starts_with('$') {
                        TokenType::COMMENT_PREPROC
                    } else {
                        TokenType::COMMENT_MULTILINE
                    };
                } else if sc.scan(&re.single) {
                    token = TokenType::COMMENT_SINGLE;
                } else if portugol && sc.scan(&re.port_op) {
                    token = TokenType::OPERATOR;
                } else if !portugol && sc.scan(&re.op) {
                    token = TokenType::OPERATOR;
                    // stop label highlighting on next ";"
                    if collect_labels && sc.matched() == ";" {
                        collect_labels = false;
                    }
                } else if sc.scan(&re.punct) {
                    token = TokenType::PUNCTUATION;
                    next_token_is_function = false;
                    if in_function_block || in_property_block {
                        match sc.matched() {
                            "(" => brace_balance[0] += 1,
                            ")" => brace_balance[0] -= 1,
                            "[" => brace_balance[1] += 1,
                            "]" => brace_balance[1] -= 1,
                            _ => {}
                        }
                    }
                } else if sc.scan(&re.ident) {
                    lower.clear();
                    lower.extend(sc.matched().chars().flat_map(char::to_lowercase));
                    let name = lower.as_str();
                    if name == "result" {
                        token = TokenType::NAME_BUILTIN_PSEUDO;
                    } else if sets.keywords.contains(name) {
                        token = TokenType::KEYWORD;
                        if portugol {
                            if matches!(name, "funcao" | "procedimento") {
                                in_function_block = true;
                                next_token_is_function = true;
                            }
                        } else {
                            if (in_function_block || in_property_block)
                                && sets.block_keywords.contains(name)
                                && brace_balance[0] <= 0
                                && brace_balance[1] <= 0
                            {
                                in_function_block = false;
                                in_property_block = false;
                                brace_balance = [0, 0];
                                block_labels.clear();
                            }
                            match name {
                                "label" | "goto" => collect_labels = true,
                                "asm" => stack.push(St::Asm),
                                "property" => {
                                    in_property_block = true;
                                    next_token_is_property = true;
                                }
                                "procedure" | "operator" | "function" | "constructor" | "destructor" => {
                                    in_function_block = true;
                                    next_token_is_function = true;
                                }
                                _ => {}
                            }
                        }
                    } else if !portugol && in_function_block && sets.function_modifiers.contains(name) {
                        token = TokenType::KEYWORD_PSEUDO;
                    } else if !portugol && in_property_block && matches!(name, "read" | "write") {
                        token = TokenType::KEYWORD_PSEUDO;
                        next_token_is_function = true;
                    } else if next_token_is_function {
                        if !portugol && sc.test(&re.dot_test) {
                            token = TokenType::NAME_CLASS;
                        } else {
                            token = TokenType::NAME_FUNCTION;
                            next_token_is_function = false;
                            if portugol {
                                block_labels.insert(name.to_owned());
                            }
                        }
                    } else if !portugol && next_token_is_property {
                        token = TokenType::NAME_PROPERTY;
                        next_token_is_property = false;
                    } else if !portugol && collect_labels {
                        token = TokenType::NAME_LABEL;
                        block_labels.insert(name.to_owned());
                    } else if block_labels.contains(name) {
                        token = TokenType::NAME_LABEL;
                    } else if portugol && sets.portugol_builtin_types.contains(name) {
                        token = TokenType::KEYWORD_TYPE;
                    } else if !portugol && sets.builtin_types.contains(name) {
                        token = TokenType::KEYWORD_TYPE;
                    } else if !portugol && sets.directives.contains(name) {
                        token = TokenType::KEYWORD_PSEUDO;
                    } else if !portugol && !was_dot && sets.builtins.contains(name) {
                        token = TokenType::NAME_BUILTIN;
                    } else {
                        token = TokenType::NAME;
                    }
                } else if portugol && sc.scan(&re.dq) {
                    token = TokenType::LITERAL_STRING;
                    stack.push(St::Str);
                } else if !portugol && sc.scan(&re.sq) {
                    token = TokenType::LITERAL_STRING;
                    stack.push(St::Str);
                } else if !portugol && sc.scan(&re.chr) {
                    token = TokenType::LITERAL_STRING_CHAR;
                } else if !portugol && sc.scan(&re.hex) {
                    token = TokenType::LITERAL_NUMBER_HEX;
                } else if sc.scan(&re.int) {
                    token = TokenType::LITERAL_NUMBER_INTEGER;
                } else if sc.scan(&re.float) {
                    token = TokenType::LITERAL_NUMBER_FLOAT;
                } else {
                    if stack.len() > 1 {
                        stack.pop();
                    }
                    sc.get_char();
                }
            }
            St::Str => {
                let (quote, rest) = if portugol { (&re.dq, &re.not_dq) } else { (&re.sq, &re.not_sq) };
                if sc.scan(&re.esc) {
                    token = TokenType::LITERAL_STRING_ESCAPE;
                } else if sc.scan(quote) {
                    token = TokenType::LITERAL_STRING;
                    stack.pop();
                } else if sc.scan(rest) {
                    token = TokenType::LITERAL_STRING;
                } else {
                    sc.get_char();
                    stack.pop();
                }
            }
            St::Asm => {
                if sc.scan(&re.ws) {
                    token = TokenType::TEXT_WHITESPACE;
                } else if sc.scan(&re.end) {
                    token = TokenType::KEYWORD;
                    stack.pop();
                } else if sc.scan(&re.comment) {
                    token = if sc.matched().starts_with('$') {
                        TokenType::COMMENT_PREPROC
                    } else {
                        TokenType::COMMENT_MULTILINE
                    };
                } else if sc.scan(&re.single) {
                    token = TokenType::COMMENT_SINGLE;
                } else if sc.scan(&re.sq) {
                    token = TokenType::LITERAL_STRING;
                    stack.push(St::Str);
                } else if sc.scan(&re.asm_label) {
                    token = TokenType::NAME_LABEL;
                } else if sc.scan(&re.ident) {
                    lower.clear();
                    lower.extend(sc.matched().chars().flat_map(char::to_lowercase));
                    token = if sets.asm_instructions.contains(lower.as_str()) {
                        TokenType::KEYWORD
                    } else if sets.asm_registers.contains(lower.as_str()) {
                        TokenType::NAME_BUILTIN
                    } else {
                        TokenType::NAME
                    };
                } else if sc.scan(&re.asm_op) {
                    token = TokenType::OPERATOR;
                } else if sc.scan(&re.punct) {
                    token = TokenType::PUNCTUATION;
                } else if sc.scan(&re.hex) {
                    token = TokenType::LITERAL_NUMBER_HEX;
                } else if sc.scan(&re.int) {
                    token = TokenType::LITERAL_NUMBER_INTEGER;
                } else if sc.scan(&re.float) {
                    token = TokenType::LITERAL_NUMBER_FLOAT;
                } else {
                    sc.get_char();
                    stack.pop();
                }
            }
        }
        // save the dot!!!11
        let m = sc.matched();
        if !portugol && !py_strip(m).is_empty() {
            was_dot = m == ".";
        }
        out.push(Tok::new(token, sc.start, sc.end));
        if sc.end == sc.start && sc.pos == sc.start {
            // An empty match would loop forever (as in Python); bail out.
            if sc.pos < text.len() {
                out.push(Tok::new(TokenType::ERROR, sc.pos, text.len()));
            }
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{exp, lex};

    #[test]
    fn delphi_function() {
        assert_eq!(
            lex("delphi", "function TFoo.Bar(x: Integer): string; cdecl;\nbegin Result := 'a''b' + #13; end;\n"),
            exp(&[
                ("Keyword", "function"),
                ("Text.Whitespace", " "),
                ("Name.Class", "TFoo"),
                ("Operator", "."),
                ("Name.Function", "Bar"),
                ("Punctuation", "("),
                ("Name", "x"),
                ("Operator", ":"),
                ("Text.Whitespace", " "),
                ("Keyword.Type", "Integer"),
                ("Punctuation", ")"),
                ("Operator", ":"),
                ("Text.Whitespace", " "),
                ("Keyword", "string"),
                ("Operator", ";"),
                ("Text.Whitespace", " "),
                ("Keyword.Pseudo", "cdecl"),
                ("Operator", ";"),
                ("Text.Whitespace", "\n"),
                ("Keyword", "begin"),
                ("Text.Whitespace", " "),
                ("Name.Builtin.Pseudo", "Result"),
                ("Text.Whitespace", " "),
                ("Operator", ":"),
                ("Operator", "="),
                ("Text.Whitespace", " "),
                ("Literal.String", "'"),
                ("Literal.String", "a"),
                ("Literal.String.Escape", "''"),
                ("Literal.String", "b"),
                ("Literal.String", "'"),
                ("Text.Whitespace", " "),
                ("Operator", "+"),
                ("Text.Whitespace", " "),
                ("Literal.String.Char", "#13"),
                ("Operator", ";"),
                ("Text.Whitespace", " "),
                ("Keyword", "end"),
                ("Operator", ";"),
                ("Text.Whitespace", "\n"),
            ])
        );
    }

    #[test]
    fn delphi_asm_labels() {
        assert_eq!(
            lex("delphi", "label l1;\nasm mov eax, $1F end;\ngoto l1; {c} (*d*) // e\n"),
            exp(&[
                ("Keyword", "label"),
                ("Text.Whitespace", " "),
                ("Name.Label", "l1"),
                ("Operator", ";"),
                ("Text.Whitespace", "\n"),
                ("Keyword", "asm"),
                ("Text.Whitespace", " "),
                ("Keyword", "mov"),
                ("Text.Whitespace", " "),
                ("Name.Builtin", "eax"),
                ("Operator", ","),
                ("Text.Whitespace", " "),
                ("Literal.Number.Hex", "$1F"),
                ("Text.Whitespace", " "),
                ("Keyword", "end"),
                ("Operator", ";"),
                ("Text.Whitespace", "\n"),
                ("Keyword", "goto"),
                ("Text.Whitespace", " "),
                ("Name.Label", "l1"),
                ("Operator", ";"),
                ("Text.Whitespace", " "),
                ("Comment.Multiline", "{c}"),
                ("Text.Whitespace", " "),
                ("Comment.Multiline", "(*d*)"),
                ("Text.Whitespace", " "),
                ("Comment.Single", "// e"),
                ("Text.Whitespace", "\n"),
            ])
        );
    }

    #[test]
    fn portugol() {
        assert_eq!(
            lex("portugol", "funcao f(x: inteiro)\ninicio\n  f <- \"s\" + 1.5\n"),
            exp(&[
                ("Keyword", "funcao"),
                ("Text.Whitespace", " "),
                ("Name.Function", "f"),
                ("Punctuation", "("),
                ("Name", "x"),
                ("Operator", ":"),
                ("Text.Whitespace", " "),
                ("Keyword.Type", "inteiro"),
                ("Punctuation", ")"),
                ("Text.Whitespace", "\n"),
                ("Keyword", "inicio"),
                ("Text.Whitespace", "\n  "),
                ("Name.Label", "f"),
                ("Text.Whitespace", " "),
                ("Operator", "<-"),
                ("Text.Whitespace", " "),
                ("Literal.String", "\""),
                ("Literal.String", "s"),
                ("Literal.String", "\""),
                ("Text.Whitespace", " "),
                ("Operator", "+"),
                ("Text.Whitespace", " "),
                ("Literal.Number.Float", "1.5"),
                ("Text.Whitespace", "\n"),
            ])
        );
    }
}
