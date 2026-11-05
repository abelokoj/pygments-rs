//! Hand-ported console/session lexers (prompt + code + output), the literate
//! lexers and the trivial special lexers (`TextLexer`, `OutputLexer`,
//! `RawTokenLexer`).
//!
//! Python builds a separate code buffer per block, lexes it and splices the
//! prompts back in with `do_insertions`. The positions Python yields for such
//! blocks are often not offsets into the text (the values are what matter), so
//! [`Block`] records where each piece came from and maps the spliced tokens
//! back onto the text.

use std::sync::LazyLock;

use onig::{Regex, Region, SearchOptions};

use super::{Module, do_insertions, lex_sub};
use crate::engine::{Tok, compile};
use crate::lexer::LexerImpl;
use crate::registry::by_class;
use crate::token::TokenType;

pub(crate) static MODULE: Module = Module {
    callbacks: &[],
    filters: &[],
    custom: &[
        ("BashSessionLexer", bash_session),
        ("MSDOSSessionLexer", msdos_session),
        ("PowerShellSessionLexer", powershell_session),
        ("TcshSessionLexer", tcsh_session),
        ("DylanConsoleLexer", dylan_console),
        ("ElixirConsoleLexer", elixir_console),
        ("ErlangShellLexer", erlang_shell),
        ("GAPConsoleLexer", gap_console),
        ("JuliaConsoleLexer", julia_console),
        ("MatlabSessionLexer", matlab_session),
        ("NodeConsoleLexer", node_console),
        ("PostgresConsoleLexer", postgres_console),
        ("PsyshConsoleLexer", psysh_console),
        ("RConsoleLexer", r_console),
        ("RubyConsoleLexer", ruby_console),
        ("SqliteConsoleLexer", sqlite_console),
        ("LiterateAgdaLexer", literate_agda),
        ("LiterateCryptolLexer", literate_cryptol),
        ("LiterateHaskellLexer", literate_haskell),
        ("LiterateIdrisLexer", literate_idris),
        ("TextLexer", text_lexer),
        ("OutputLexer", output_lexer),
        ("RawTokenLexer", raw_token_lexer),
    ],
    analyse: &[],
};

// -- shared helpers ------------------------------------------------------------------

fn re(pattern: &str) -> Regex {
    compile(pattern, "").unwrap_or_else(|e| panic!("bad built-in regex {pattern:?}: {e}"))
}

/// `pattern.match(s)`: length of the match at the start of `s`.
fn match_len(re: &Regex, s: &str) -> Option<usize> {
    re.match_with_options(s, 0, SearchOptions::SEARCH_OPTION_NONE, None)
}

/// `pattern.match(s)` filling `region` with the groups.
fn match_groups(re: &Regex, s: &str, region: &mut Region) -> bool {
    re.match_with_options(s, 0, SearchOptions::SEARCH_OPTION_NONE, Some(region)).is_some()
}

/// Python's `str.isspace()` per character.
fn py_ws(c: char) -> bool {
    c.is_whitespace() || ('\x1c'..='\x1f').contains(&c)
}

/// Byte ranges of `line_re.finditer(text)` (`.*?\n`): text after the last
/// newline is not part of any line, exactly as in Pygments.
fn lines(text: &str) -> impl Iterator<Item = (usize, usize)> + '_ {
    let mut start = 0;
    text.match_indices('\n').map(move |(i, _)| {
        let s = start;
        start = i + 1;
        (s, i + 1)
    })
}

/// Byte ranges of Python's `text.splitlines(keepends=True)`.
fn splitlines(text: &str) -> Vec<(usize, usize)> {
    let mut v = Vec::new();
    let mut start = 0;
    let mut it = text.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        let end = match c {
            '\r' if matches!(it.peek(), Some(&(_, '\n'))) => {
                it.next();
                i + 2
            }
            '\n' | '\r' | '\x0b' | '\x0c' | '\x1c' | '\x1d' | '\x1e' | '\u{85}' | '\u{2028}' | '\u{2029}' => {
                i + c.len_utf8()
            }
            _ => continue,
        };
        v.push((start, end));
        start = end;
    }
    if start < text.len() {
        v.push((start, text.len()));
    }
    v
}

/// Byte length of the first `n` characters of `s` (Python `s[:n]`).
fn char_prefix(s: &str, n: usize) -> usize {
    s.char_indices().nth(n).map_or(s.len(), |(i, _)| i)
}

/// Lex `text[start..end]` with `lexer` (plain text if it is missing).
fn lex_into(lexer: Option<&'static LexerImpl>, text: &str, start: usize, end: usize, out: &mut Vec<Tok>) {
    match lexer {
        Some(l) => lex_sub(l, text, start, end, None, out),
        None if start < end => out.push(Tok::new(TokenType::TEXT, start, end)),
        None => {}
    }
}

/// The nearest token type Pygments' dotted `name` (without `Token.`) or one of
/// its ancestors names; Python creates unknown types on the fly.
fn known_type(mut name: &str) -> TokenType {
    loop {
        if let Some(t) = TokenType::from_name(name) {
            return t;
        }
        match name.rfind('.') {
            Some(i) => name = &name[..i],
            None => return TokenType::TOKEN,
        }
    }
}

/// One `curcode` + `insertions` block of a Python console lexer.
#[derive(Default)]
struct Block {
    code: String,
    ins: Vec<(usize, Vec<Tok>)>,
    /// `(virtual offset, text offset)` wherever the pieces, in the order
    /// `do_insertions` emits them, stop being contiguous in the text.
    map: Vec<(usize, usize)>,
    vlen: usize,
}

impl Block {
    fn seg(&mut self, start: usize, len: usize) {
        let vlen = self.vlen;
        if self.map.last().is_none_or(|&(v, t)| t + (vlen - v) != start) {
            self.map.push((vlen, start));
        }
        self.vlen += len;
    }

    /// Insert `toks` (only their lengths matter) covering `len` bytes at `start`.
    fn insert(&mut self, toks: Vec<Tok>, start: usize, len: usize) {
        self.ins.push((self.code.len(), toks));
        self.seg(start, len);
    }

    fn prompt(&mut self, ty: TokenType, start: usize, end: usize) {
        self.insert(vec![Tok::new(ty, start, end)], start, end - start);
    }

    fn code(&mut self, text: &str, start: usize, end: usize) {
        self.code.push_str(&text[start..end]);
        self.seg(start, end - start);
    }

    /// Text of the last token of the last insertion (`insertions[-1][-1][-1]`).
    fn last_inserted<'t>(&self, text: &'t str) -> Option<&'t str> {
        let t = self.ins.last()?.1.last()?;
        text.get(t.start..t.end)
    }

    /// `do_insertions(insertions, lexer.get_tokens_unprocessed(curcode))`, then reset.
    fn flush(&mut self, lexer: Option<&'static LexerImpl>, stack: Option<&[&str]>, out: &mut Vec<Tok>) {
        let mut toks = Vec::new();
        match lexer {
            Some(l) => l.tokens_unprocessed(&self.code, stack, &mut toks),
            None if !self.code.is_empty() => toks.push(Tok::new(TokenType::TEXT, 0, self.code.len())),
            None => {}
        }
        let mut k = 0;
        for mut t in do_insertions(std::mem::take(&mut self.ins), toks) {
            // do_insertions splits tokens at every insertion, and every gap in
            // the text is at an insertion, so no token straddles a gap.
            while k + 1 < self.map.len() && self.map[k + 1].0 <= t.start {
                k += 1;
            }
            let (v, s) = self.map.get(k).copied().unwrap_or((0, 0));
            t.start = t.start - v + s;
            t.end = t.end - v + s;
            out.push(t);
        }
        self.code.clear();
        self.map.clear();
        self.vlen = 0;
    }
}

// -- shell sessions (shell.py ShellSessionBaseLexer) -----------------------------------

static VENV: LazyLock<Regex> = LazyLock::new(|| re(r"^(\([^)]*\))(\s*)"));
static VENV_TYPE: LazyLock<TokenType> = LazyLock::new(|| known_type("Generic.Prompt.VirtualEnv"));
static BASH_PS1: LazyLock<Regex> = LazyLock::new(|| {
    re(r"^((?:(?:\[.*?\])|(?:\(\S+\))?(?:| |sh\S*?|\w+\S+[@:]\S+(?:\s+\S+)?|\[\S+[@:][^\n]+\].+))\s*[$#%]\s*)(.*\n?)")
});
static MSDOS_PS1: LazyLock<Regex> = LazyLock::new(|| re(r"^([^>]*>)(.*\n?)"));
static TCSH_PS1: LazyLock<Regex> = LazyLock::new(|| re(r"^([^>]+>)(.*\n?)"));
static PWSH_PS1: LazyLock<Regex> = LazyLock::new(|| re(r"^((?:\[[^\]]+\]: )?PS[^>]*> ?)(.*\n?)"));

struct Shell {
    inner: &'static str,
    ps1: &'static LazyLock<Regex>,
    ps2: &'static str,
    bare_continuation: bool,
}

fn bash_session(_: &'static LexerImpl, text: &str, out: &mut Vec<Tok>) {
    let cfg = Shell { inner: "BashLexer", ps1: &BASH_PS1, ps2: "> ", bare_continuation: false };
    shell_session(&cfg, text, out);
}

fn msdos_session(_: &'static LexerImpl, text: &str, out: &mut Vec<Tok>) {
    let cfg = Shell { inner: "BatchLexer", ps1: &MSDOS_PS1, ps2: "More? ", bare_continuation: false };
    shell_session(&cfg, text, out);
}

fn tcsh_session(_: &'static LexerImpl, text: &str, out: &mut Vec<Tok>) {
    let cfg = Shell { inner: "TcshLexer", ps1: &TCSH_PS1, ps2: "? ", bare_continuation: false };
    shell_session(&cfg, text, out);
}

fn powershell_session(_: &'static LexerImpl, text: &str, out: &mut Vec<Tok>) {
    let cfg = Shell { inner: "PowerShellLexer", ps1: &PWSH_PS1, ps2: "> ", bare_continuation: true };
    shell_session(&cfg, text, out);
}

fn shell_session(cfg: &Shell, text: &str, out: &mut Vec<Tok>) {
    let inner = by_class(cfg.inner);
    let ps1: &Regex = cfg.ps1;
    let ps2 = cfg.ps2;
    let mut b = Block::default();
    let mut region = Region::new();
    let mut backslash = false;
    for (ls, le) in lines(text) {
        let mut s = ls;
        if let (true, Some((v0, v1)), Some((_, w1))) =
            (match_groups(&VENV, &text[ls..le], &mut region), region.pos(1), region.pos(2))
        {
            b.prompt(*VENV_TYPE, ls + v0, ls + v1);
            if w1 > v1 {
                b.prompt(TokenType::TEXT, ls + v1, ls + w1);
            }
            s = ls + w1;
        }
        let line = &text[s..le];
        if let (true, Some((_, p1)), Some((c0, c1))) =
            (match_groups(ps1, line, &mut region), region.pos(1), region.pos(2))
        {
            b.prompt(TokenType::GENERIC_PROMPT, s, s + p1);
            b.code(text, s + c0, s + c1);
            backslash = b.code.ends_with("\\\n");
        } else if backslash {
            if line.starts_with(ps2) {
                b.prompt(TokenType::GENERIC_PROMPT, s, s + ps2.len());
                b.code(text, s + ps2.len(), le);
            } else {
                b.code(text, s, le);
            }
            backslash = b.code.ends_with("\\\n");
        } else if cfg.bare_continuation && line.starts_with(ps2) {
            b.prompt(TokenType::GENERIC_PROMPT, s, s + ps2.len());
            b.code(text, s + ps2.len(), le);
        } else {
            if !b.ins.is_empty() {
                b.flush(inner, None, out);
            }
            out.push(Tok::new(TokenType::GENERIC_OUTPUT, s, le));
        }
    }
    if !b.ins.is_empty() {
        b.flush(inner, None, out);
    }
}

// -- prompt-regex consoles ------------------------------------------------------------

/// The common loop: a prompt line puts its prompt and code in the block, any
/// other line flushes the block (if it has code) and is output.
fn console(
    text: &str,
    out: &mut Vec<Tok>,
    inner: &str,
    stack: Option<&[&str]>,
    prompt: impl Fn(&str) -> Option<usize>,
    output: impl Fn(&str) -> TokenType,
) {
    let inner = by_class(inner);
    let mut b = Block::default();
    for (s, e) in lines(text) {
        let line = &text[s..e];
        if let Some(p) = prompt(line) {
            b.prompt(TokenType::GENERIC_PROMPT, s, s + p);
            b.code(text, s + p, e);
        } else {
            if !b.code.is_empty() {
                b.flush(inner, stack, out);
            }
            out.push(Tok::new(output(line), s, e));
        }
    }
    if !b.code.is_empty() {
        b.flush(inner, stack, out);
    }
}

static DYLAN_PROMPT: LazyLock<Regex> = LazyLock::new(|| re(r"\?| "));
static ERLANG_PROMPT: LazyLock<Regex> = LazyLock::new(|| re(r"(?:\([\w@_.]+\))?\d+>(?=\s|\Z)"));
static ELIXIR_PROMPT: LazyLock<Regex> = LazyLock::new(|| re(r"(iex|\.{3})((?:\([\w@_.]+\))?\d+|\(\d+\))?> "));
static RUBY_PROMPT: LazyLock<Regex> = LazyLock::new(|| re(r#"irb\([a-zA-Z_]\w*\):\d{3}:\d+[>*"\'] |>> |\?> "#));

fn dylan_console(_: &'static LexerImpl, text: &str, out: &mut Vec<Tok>) {
    console(text, out, "DylanLexer", None, |l| match_len(&DYLAN_PROMPT, l), |_| TokenType::GENERIC_OUTPUT);
}

fn ruby_console(_: &'static LexerImpl, text: &str, out: &mut Vec<Tok>) {
    console(text, out, "RubyLexer", None, |l| match_len(&RUBY_PROMPT, l), |_| TokenType::GENERIC_OUTPUT);
}

fn erlang_shell(_: &'static LexerImpl, text: &str, out: &mut Vec<Tok>) {
    console(
        text,
        out,
        "ErlangLexer",
        None,
        |l| match_len(&ERLANG_PROMPT, l),
        |l| if l.starts_with('*') { TokenType::GENERIC_TRACEBACK } else { TokenType::GENERIC_OUTPUT },
    );
}

fn psysh_console(_: &'static LexerImpl, text: &str, out: &mut Vec<Tok>) {
    // PsyshConsoleLexer forces `startinline`, so PhpLexer starts in 'php'.
    console(
        text,
        out,
        "PhpLexer",
        Some(&["root", "php"][..]),
        |l| {
            if l.starts_with(">>> ") || l.starts_with("... ") {
                Some(4)
            } else if l.trim_end_matches(py_ws) == "..." {
                Some(3)
            } else {
                None
            }
        },
        |_| TokenType::GENERIC_OUTPUT,
    );
}

fn r_console(_: &'static LexerImpl, text: &str, out: &mut Vec<Tok>) {
    console(
        text,
        out,
        "SLexer",
        None,
        |l| (l.starts_with('>') || l.starts_with('+')).then(|| char_prefix(l, 2)),
        |_| TokenType::GENERIC_OUTPUT,
    );
}

fn elixir_console(_: &'static LexerImpl, text: &str, out: &mut Vec<Tok>) {
    let inner = by_class("ElixirLexer");
    let mut b = Block::default();
    let mut in_error = false;
    for (s, e) in lines(text) {
        let line = &text[s..e];
        if line.starts_with("** ") {
            in_error = true;
            // Lines end with "\n", so `line[:-1]` drops one byte.
            b.prompt(TokenType::GENERIC_ERROR, s, e - 1);
            b.code(text, e - 1, e);
        } else if let Some(p) = match_len(&ELIXIR_PROMPT, line) {
            in_error = false;
            b.prompt(TokenType::GENERIC_PROMPT, s, s + p);
            b.code(text, s + p, e);
        } else {
            if !b.code.is_empty() {
                b.flush(inner, None, out);
            }
            let ty = if in_error { TokenType::GENERIC_ERROR } else { TokenType::GENERIC_OUTPUT };
            out.push(Tok::new(ty, s, e));
        }
    }
    if !b.code.is_empty() {
        b.flush(inner, None, out);
    }
}

static SQLITE_PROMPT: LazyLock<Regex> = LazyLock::new(|| re(r"^(?:sqlite|   ...)>(?= )"));

fn sqlite_console(_: &'static LexerImpl, text: &str, out: &mut Vec<Tok>) {
    let inner = by_class("SqlLexer");
    let mut b = Block::default();
    for (s, e) in lines(text) {
        let line = &text[s..e];
        if let Some(p) = match_len(&SQLITE_PROMPT, line) {
            // Python's `line[:7]`; the lookahead guarantees a space after it.
            b.prompt(TokenType::GENERIC_PROMPT, s, s + p);
            b.prompt(TokenType::TEXT_WHITESPACE, s + p, s + p + 1);
            b.code(text, s + p + 1, e);
        } else {
            if !b.code.is_empty() {
                b.flush(inner, None, out);
            }
            let ty =
                if line.starts_with("SQL error: ") { TokenType::GENERIC_TRACEBACK } else { TokenType::GENERIC_OUTPUT };
            out.push(Tok::new(ty, s, e));
        }
    }
    if !b.code.is_empty() {
        b.flush(inner, None, out);
    }
}

// -- GAP / Julia (splitlines based) -----------------------------------------------------

fn gap_console(_: &'static LexerImpl, text: &str, out: &mut Vec<Tok>) {
    let inner = by_class("GAPLexer");
    let mut b = Block::default();
    let (mut output, mut error) = (false, false);
    for (s, e) in splitlines(text) {
        let line = &text[s..e];
        if line.starts_with("gap> ") || line.starts_with("brk> ") {
            b.prompt(TokenType::GENERIC_PROMPT, s, s + 5);
            b.code(text, s + 5, e);
            output = false;
            error = false;
        } else if !output && line.starts_with("> ") {
            b.prompt(TokenType::GENERIC_PROMPT, s, s + 2);
            b.code(text, s + 2, e);
        } else {
            if !b.code.is_empty() {
                b.flush(inner, None, out);
            }
            if line.starts_with("Error, ") || error {
                out.push(Tok::new(TokenType::GENERIC_ERROR, s, e));
                error = true;
            } else {
                out.push(Tok::new(TokenType::GENERIC_OUTPUT, s, e));
            }
            output = true;
        }
    }
    if !b.code.is_empty() {
        b.flush(inner, None, out);
    }
}

fn julia_console(_: &'static LexerImpl, text: &str, out: &mut Vec<Tok>) {
    let inner = by_class("JuliaLexer");
    let mut b = Block::default();
    let (mut output, mut error) = (false, false);
    for (s, e) in splitlines(text) {
        let line = &text[s..e];
        if line.starts_with("julia>") {
            b.prompt(TokenType::GENERIC_PROMPT, s, s + 6);
            b.code(text, s + 6, e);
            output = false;
            error = false;
        } else if line.starts_with("help?>") || line.starts_with("shell>") {
            // Emitted before any pending code block, as in Python.
            out.push(Tok::new(TokenType::GENERIC_PROMPT, s, s + 6));
            out.push(Tok::new(TokenType::TEXT, s + 6, e));
            output = false;
            error = false;
        } else if line.starts_with("      ") && !output {
            b.prompt(TokenType::TEXT_WHITESPACE, s, s + 6);
            b.code(text, s + 6, e);
        } else {
            if !b.code.is_empty() {
                b.flush(inner, None, out);
            }
            if line.starts_with("ERROR: ") || error {
                out.push(Tok::new(TokenType::GENERIC_ERROR, s, e));
                error = true;
            } else {
                out.push(Tok::new(TokenType::GENERIC_OUTPUT, s, e));
            }
            output = true;
        }
    }
    if !b.code.is_empty() {
        b.flush(inner, None, out);
    }
}

// -- Matlab ---------------------------------------------------------------------------

fn matlab_session(_: &'static LexerImpl, text: &str, out: &mut Vec<Tok>) {
    let inner = by_class("MatlabLexer");
    let mut b = Block::default();
    let mut continuation = false;
    for (s, e) in lines(text) {
        let line = &text[s..e];
        if line.starts_with(">> ") {
            b.prompt(TokenType::GENERIC_PROMPT, s, s + 3);
            b.code(text, s + 3, e);
        } else if line.starts_with(">>") {
            b.prompt(TokenType::GENERIC_PROMPT, s, s + 2);
            b.code(text, s + 2, e);
        } else if line.starts_with("???") {
            b.prompt(TokenType::GENERIC_TRACEBACK, s, e);
        } else if continuation && !b.ins.is_empty() {
            // Leading spaces as wide as the last prompt count as a prompt.
            let n = b.last_inserted(text).map_or(0, |v| v.chars().count());
            if line.len() >= n && line.as_bytes()[..n].iter().all(|&c| c == b' ') {
                b.prompt(TokenType::GENERIC_PROMPT, s, s + n);
                b.code(text, s + n, e);
            } else {
                b.code(text, s, e);
            }
        } else {
            // Python keeps `insertions` when there is no code (a quirk that
            // delays or drops "???" lines); so does `flush` only on code.
            if !b.code.is_empty() {
                b.flush(inner, None, out);
            }
            out.push(Tok::new(TokenType::GENERIC_OUTPUT, s, e));
        }
        continuation = line.trim_matches(py_ws).ends_with("...");
    }
    if !b.code.is_empty() {
        b.flush(inner, None, out);
    }
}

// -- Node.js --------------------------------------------------------------------------

fn node_console(_: &'static LexerImpl, text: &str, out: &mut Vec<Tok>) {
    let inner = by_class("JavascriptLexer");
    let mut b = Block::default();
    for (s, e) in lines(text) {
        let line = &text[s..e];
        if line.starts_with("> ") {
            let toks =
                vec![Tok::new(TokenType::GENERIC_PROMPT, s, s + 1), Tok::new(TokenType::TEXT_WHITESPACE, s + 1, s + 2)];
            b.insert(toks, s, 2);
            b.code(text, s + 2, e);
        } else if line.starts_with("...") {
            let lead = line.len() - line.trim_start_matches('.').len();
            b.prompt(TokenType::GENERIC_PROMPT, s, s + lead);
            b.code(text, s + lead, e);
        } else {
            if !b.code.is_empty() {
                b.flush(inner, None, out);
            }
            lex_into(inner, text, s, e, out);
        }
    }
    if !b.code.is_empty() {
        b.flush(inner, None, out);
    }
}

// -- psql (sql.py PostgresConsoleLexer) -----------------------------------------------

static PSQL_PROMPT: LazyLock<Regex> = LazyLock::new(|| re(r#"^(\S.*?)??[=\-\(\$\'\"][#>]"#));
static PSQL_COMMAND: LazyLock<Regex> = LazyLock::new(|| re(r"(\s*)(\\.+?)(\s+)$"));
static PSQL_END_COMMAND: LazyLock<Regex> = LazyLock::new(|| re(r";\s*(--.*?)?$"));
static PSQL_MESSAGE: LazyLock<Regex> =
    LazyLock::new(|| re(r"((?:DEBUG|INFO|NOTICE|WARNING|ERROR|FATAL|HINT|DETAIL|CONTEXT|LINE [0-9]+):)(.*?\n)"));

fn postgres_console(_: &'static LexerImpl, text: &str, out: &mut Vec<Tok>) {
    // shortcut: PsqlRegexLexer is not in the registry yet; PostgresLexer
    // differs only on psql backslash commands. Drops out once it is added.
    let sql = by_class("PsqlRegexLexer").or_else(|| by_class("PostgresLexer"));
    let console = by_class("BashSessionLexer");
    let lines: Vec<(usize, usize)> = lines(text).collect();
    let mut region = Region::new();
    let mut i = 0;
    loop {
        // The command: an optional prompt, continued until its end is seen.
        let mut b = Block::default();
        while let Some(&(s, e)) = lines.get(i) {
            i += 1;
            let line = &text[s..e];
            if line.starts_with('$') && b.code.is_empty() {
                lex_into(console, text, s, e, out);
                break;
            }
            match match_len(&PSQL_PROMPT, line) {
                Some(p) => {
                    b.prompt(TokenType::GENERIC_PROMPT, s, s + p);
                    b.code(text, s + p, e);
                }
                None => b.code(text, s, e),
            }
            let code = b.code.as_str();
            if match_len(&PSQL_COMMAND, code).is_some()
                || PSQL_END_COMMAND
                    .search_with_options(code, 0, code.len(), SearchOptions::SEARCH_OPTION_NONE, None)
                    .is_some()
            {
                break;
            }
        }
        b.flush(sql, None, out);
        // The output, up to the next prompt (which is pushed back).
        let mut out_token = TokenType::GENERIC_OUTPUT;
        loop {
            let Some(&(s, e)) = lines.get(i) else { return };
            let line = &text[s..e];
            if match_len(&PSQL_PROMPT, line).is_some() {
                break;
            }
            i += 1;
            if let (true, Some((a0, a1)), Some((b0, b1))) =
                (match_groups(&PSQL_MESSAGE, line, &mut region), region.pos(1), region.pos(2))
            {
                let label = &line[a0..a1];
                if label.starts_with("ERROR") || label.starts_with("FATAL") {
                    out_token = TokenType::GENERIC_ERROR;
                }
                out.push(Tok::new(TokenType::GENERIC_STRONG, s + a0, s + a1));
                out.push(Tok::new(out_token, s + b0, s + b1));
            } else {
                out.push(Tok::new(out_token, s, e));
            }
        }
    }
}

// -- literate lexers (haskell.py LiterateLexer) -----------------------------------------

static BIRD: LazyLock<Regex> = LazyLock::new(|| re(r"(>[ \t]*)(.*\n)"));

fn literate_haskell(_: &'static LexerImpl, text: &str, out: &mut Vec<Tok>) {
    literate("HaskellLexer", false, text, out);
}

fn literate_idris(_: &'static LexerImpl, text: &str, out: &mut Vec<Tok>) {
    literate("IdrisLexer", false, text, out);
}

fn literate_agda(_: &'static LexerImpl, text: &str, out: &mut Vec<Tok>) {
    literate("AgdaLexer", true, text, out);
}

fn literate_cryptol(_: &'static LexerImpl, text: &str, out: &mut Vec<Tok>) {
    literate("CryptolLexer", false, text, out);
}

fn literate(base: &str, force_latex: bool, text: &str, out: &mut Vec<Tok>) {
    let base = by_class(base);
    // `text.lstrip()[0:1] in '%\\'` is also true for an empty string.
    let latex = force_latex || matches!(text.trim_start_matches(py_ws).chars().next(), None | Some('%' | '\\'));
    let mut b = Block::default();
    if latex {
        let tex = by_class("TexLexer");
        let insert_tex = |b: &mut Block, start: usize, end: usize| {
            let mut toks = Vec::new();
            lex_into(tex, text, start, end, &mut toks);
            b.insert(toks, start, end - start);
        };
        let mut in_code = false;
        let mut latex_start = 0;
        let mut last_end = 0;
        for (s, e) in lines(text) {
            last_end = e;
            let stripped = text[s..e].trim_start_matches(py_ws);
            if in_code {
                if stripped.starts_with("\\end{code}") {
                    in_code = false;
                    latex_start = s;
                } else {
                    b.code(text, s, e);
                }
            } else if stripped.starts_with("\\begin{code}") {
                in_code = true;
                insert_tex(&mut b, latex_start, e);
                latex_start = e;
            }
        }
        let latex_end = if in_code { latex_start } else { last_end };
        insert_tex(&mut b, latex_start, latex_end);
    } else {
        let mut region = Region::new();
        for (s, e) in lines(text) {
            if let (true, Some((_, p1)), Some((c0, c1))) =
                (match_groups(&BIRD, &text[s..e], &mut region), region.pos(1), region.pos(2))
            {
                b.prompt(TokenType::COMMENT_SPECIAL, s, s + p1);
                b.code(text, s + c0, s + c1);
            } else {
                b.prompt(TokenType::TEXT, s, e);
            }
        }
    }
    b.flush(base, None, out);
}

// -- special.py -----------------------------------------------------------------------

fn text_lexer(_: &'static LexerImpl, text: &str, out: &mut Vec<Tok>) {
    out.push(Tok::new(TokenType::TEXT, 0, text.len()));
}

fn output_lexer(_: &'static LexerImpl, text: &str, out: &mut Vec<Tok>) {
    out.push(Tok::new(TokenType::GENERIC_OUTPUT, 0, text.len()));
}

/// Each line of `Token.Type\t'repr'` becomes one token of that type; bad
/// lines are `Error`. Python yields the decoded string as the value, which a
/// span into the text cannot express, so the token covers its whole line.
fn raw_token_lexer(_: &'static LexerImpl, text: &str, out: &mut Vec<Tok>) {
    for (s, e) in lines(text) {
        let ty = parse_raw_line(text[s..e].trim_end_matches(py_ws)).unwrap_or(TokenType::ERROR);
        out.push(Tok::new(ty, s, e));
    }
}

fn parse_raw_line(line: &str) -> Option<TokenType> {
    let (name, val) = line.split_once('\t')?;
    // Python ignores the first dotted component and requires capitalized parts.
    let rest = name.split_once('.').map(|(_, r)| r);
    if rest.is_some_and(|r| !r.split('.').all(|p| p.chars().next().is_some_and(char::is_uppercase))) {
        return None;
    }
    if !is_str_literal(val) {
        return None;
    }
    Some(rest.map_or(TokenType::TOKEN, known_type))
}

/// Whether `ast.literal_eval(s)` gives a `str`: adjacent non-bytes string literals.
/// shortcut: rejects parenthesized literals and trailing `#` comments and does
/// not validate `\N{...}` names; RawTokenFormatter never writes those.
fn is_str_literal(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    let mut count = 0;
    loop {
        while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\x0c') {
            i += 1;
        }
        if i == b.len() {
            return count > 0;
        }
        let mut raw = false;
        if b[i].is_ascii_alphabetic() {
            match b[i] {
                b'r' | b'R' => raw = true,
                b'u' | b'U' => {}
                _ => return false,
            }
            i += 1;
        }
        let Some(&q) = b.get(i) else { return false };
        if q != b'\'' && q != b'"' {
            return false;
        }
        let triple = b.get(i + 1) == Some(&q) && b.get(i + 2) == Some(&q);
        let qlen = if triple { 3 } else { 1 };
        i += qlen;
        loop {
            let Some(&c) = b.get(i) else { return false };
            if c == b'\\' {
                let Some(&next) = b.get(i + 1) else { return false };
                let digits = match next {
                    _ if raw => 0,
                    b'x' => 2,
                    b'u' => 4,
                    b'U' => 8,
                    b'N' => {
                        if b.get(i + 2) != Some(&b'{') {
                            return false;
                        }
                        let Some(close) = b[i + 2..].iter().position(|&c| c == b'}') else { return false };
                        i += 2 + close + 1;
                        continue;
                    }
                    _ => 0,
                };
                let hex = b.get(i + 2..i + 2 + digits);
                if !hex.is_some_and(|h| h.iter().all(u8::is_ascii_hexdigit)) {
                    return false;
                }
                i += 2 + digits;
            } else if c == q && (!triple || (b.get(i + 1) == Some(&q) && b.get(i + 2) == Some(&q))) {
                i += qlen;
                break;
            } else if c == b'\n' && !triple {
                return false;
            } else {
                i += 1;
            }
        }
        count += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Expected tokens come from Pygments 2.19.2 `get_tokens_unprocessed`.
    fn check(alias: &str, text: &str, want: &[(&str, &str)]) {
        let lexer = crate::lexer_by_name(alias).unwrap_or_else(|| panic!("no lexer {alias}"));
        let got: Vec<(TokenType, &str)> = lexer.tokenize(text).collect();
        let want: Vec<(TokenType, &str)> = want.iter().map(|&(t, v)| (known_type(t), v)).collect();
        assert_eq!(got, want, "{alias}: {text:?}");
    }

    #[test]
    fn shell_sessions() {
        check(
            "console",
            "$ echo hi\nhi\n(venv) $ ls \\\n> -l\nout\n",
            &[
                ("Generic.Prompt", "$ "),
                ("Name.Builtin", "echo"),
                ("Text.Whitespace", " "),
                ("Text", "hi"),
                ("Text.Whitespace", "\n"),
                ("Generic.Output", "hi\n"),
                ("Generic.Prompt.VirtualEnv", "(venv)"),
                ("Text", " "),
                ("Generic.Prompt", "$ "),
                ("Text", "ls"),
                ("Text.Whitespace", " "),
                ("Literal.String.Escape", "\\\n"),
                ("Generic.Prompt", "> "),
                ("Text", "-l"),
                ("Text.Whitespace", "\n"),
                ("Generic.Output", "out\n"),
            ],
        );
        check(
            "console",
            "(venv)\n$ x\n",
            &[
                ("Generic.Prompt.VirtualEnv", "(venv)"),
                ("Text", "\n"),
                ("Generic.Output", ""),
                ("Generic.Prompt", "$ "),
                ("Text", "x"),
                ("Text.Whitespace", "\n"),
            ],
        );
        check(
            "doscon",
            "C:\\> dir\nfoo\n",
            &[
                ("Generic.Prompt", "C:\\>"),
                ("Text", " "),
                ("Keyword", "dir"),
                ("Text", "\n"),
                ("Generic.Output", "foo\n"),
            ],
        );
        check(
            "pwsh-session",
            "PS C:\\> Get-Item\n> x\nresult\n",
            &[
                ("Generic.Prompt", "PS C:\\> "),
                ("Name.Builtin", "Get-Item"),
                ("Text", "\n"),
                ("Generic.Prompt", "> "),
                ("Name", "x"),
                ("Text", "\n"),
                ("Generic.Output", "result\n"),
            ],
        );
        check(
            "tcshcon",
            "user> ls\nfile\n",
            &[("Generic.Prompt", "user>"), ("Text", " "), ("Text", "ls"), ("Text", "\n"), ("Generic.Output", "file\n")],
        );
    }

    #[test]
    fn prompt_consoles() {
        check(
            "dylan-console",
            "? 1 + 2;\n3\n",
            &[
                ("Generic.Prompt", "?"),
                ("Text.Whitespace", " "),
                ("Literal.Number.Integer", "1"),
                ("Text.Whitespace", " "),
                ("Operator", "+"),
                ("Text.Whitespace", " "),
                ("Literal.Number.Integer", "2"),
                ("Punctuation", ";"),
                ("Text.Whitespace", "\n"),
                ("Generic.Output", "3\n"),
            ],
        );
        check(
            "iex",
            "iex(1)> 1 + 1\n2\n** (Error) x\nmore\n",
            &[
                ("Generic.Prompt", "iex(1)> "),
                ("Literal.Number.Integer", "1"),
                ("Text.Whitespace", " "),
                ("Operator", "+"),
                ("Text.Whitespace", " "),
                ("Literal.Number.Integer", "1"),
                ("Text.Whitespace", "\n"),
                ("Generic.Output", "2\n"),
                ("Generic.Error", "** (Error) x"),
                ("Text.Whitespace", "\n"),
                ("Generic.Error", "more\n"),
            ],
        );
        check(
            "erl",
            "1> X = 1.\n1\n* error\n",
            &[
                ("Generic.Prompt", "1>"),
                ("Text.Whitespace", " "),
                ("Name.Variable", "X"),
                ("Text.Whitespace", " "),
                ("Operator", "="),
                ("Text.Whitespace", " "),
                ("Literal.Number.Integer", "1"),
                ("Punctuation", "."),
                ("Text.Whitespace", "\n"),
                ("Generic.Output", "1\n"),
                ("Generic.Traceback", "* error\n"),
            ],
        );
        check(
            "psysh",
            ">>> $a = 1;\n...\n=> 1\n",
            &[
                ("Generic.Prompt", ">>> "),
                ("Name.Variable", "$a"),
                ("Text", " "),
                ("Operator", "="),
                ("Text", " "),
                ("Literal.Number.Integer", "1"),
                ("Punctuation", ";"),
                ("Text", "\n"),
                ("Generic.Prompt", "..."),
                ("Text", "\n"),
                ("Generic.Output", "=> 1\n"),
            ],
        );
        check(
            "rconsole",
            "> x <- 1\n+ 2\n[1] 1\n",
            &[
                ("Generic.Prompt", "> "),
                ("Name", "x"),
                ("Text.Whitespace", " "),
                ("Operator", "<-"),
                ("Text.Whitespace", " "),
                ("Literal.Number", "1"),
                ("Text.Whitespace", "\n"),
                ("Generic.Prompt", "+ "),
                ("Literal.Number", "2"),
                ("Text.Whitespace", "\n"),
                ("Generic.Output", "[1] 1\n"),
            ],
        );
        check(
            "rbcon",
            "irb(main):001:0> 1\n=> 1\n",
            &[
                ("Generic.Prompt", "irb(main):001:0> "),
                ("Literal.Number.Integer", "1"),
                ("Text.Whitespace", "\n"),
                ("Generic.Output", "=> 1\n"),
            ],
        );
        check(
            "sqlite3",
            "sqlite> select 1;\n1\nSQL error: x\n",
            &[
                ("Generic.Prompt", "sqlite>"),
                ("Text.Whitespace", " "),
                ("Keyword", "select"),
                ("Text.Whitespace", " "),
                ("Literal.Number.Integer", "1"),
                ("Punctuation", ";"),
                ("Text.Whitespace", "\n"),
                ("Generic.Output", "1\n"),
                ("Generic.Traceback", "SQL error: x\n"),
            ],
        );
        check(
            "nodejsrepl",
            "> 1 +\n... 2\n3\n",
            &[
                ("Generic.Prompt", ">"),
                ("Text.Whitespace", " "),
                ("Literal.Number.Float", "1"),
                ("Text.Whitespace", " "),
                ("Operator", "+"),
                ("Text.Whitespace", "\n"),
                ("Generic.Prompt", "..."),
                ("Text.Whitespace", " "),
                ("Literal.Number.Float", "2"),
                ("Text.Whitespace", "\n"),
                ("Literal.Number.Float", "3"),
                ("Text.Whitespace", "\n"),
            ],
        );
    }

    #[test]
    fn gap_julia_matlab() {
        check(
            "gap-console",
            "gap> 1+1;\n2\nError, bad\nmore\n",
            &[
                ("Generic.Prompt", "gap> "),
                ("Name.Variable", "1"),
                ("Operator", "+"),
                ("Name.Variable", "1"),
                ("Operator", ";"),
                ("Text.Whitespace", "\n"),
                ("Generic.Output", "2\n"),
                ("Generic.Error", "Error, bad\n"),
                ("Generic.Error", "more\n"),
            ],
        );
        // help?> is emitted before the pending julia> block, out of text order.
        check(
            "jlcon",
            "julia> a\nhelp?> b\njulia> c\n1\nERROR: x\n",
            &[
                ("Generic.Prompt", "help?>"),
                ("Text", " b\n"),
                ("Generic.Prompt", "julia>"),
                ("Text.Whitespace", " "),
                ("Name", "a"),
                ("Text.Whitespace", "\n"),
                ("Generic.Prompt", "julia>"),
                ("Text.Whitespace", " "),
                ("Name", "c"),
                ("Text.Whitespace", "\n"),
                ("Generic.Output", "1\n"),
                ("Generic.Error", "ERROR: x\n"),
            ],
        );
        // The trailing ??? line is dropped, as in Python.
        check(
            "matlabsession",
            ">> x = 1 ...\n   + 2\nx = 3\n??? Error\n",
            &[
                ("Generic.Prompt", ">> "),
                ("Name", "x"),
                ("Text.Whitespace", " "),
                ("Punctuation", "="),
                ("Text.Whitespace", " "),
                ("Literal.Number.Integer", "1"),
                ("Text.Whitespace", " "),
                ("Keyword", "..."),
                ("Text.Whitespace", "\n"),
                ("Generic.Prompt", "   "),
                ("Operator", "+"),
                ("Text.Whitespace", " "),
                ("Literal.Number.Integer", "2"),
                ("Text.Whitespace", "\n"),
                ("Generic.Output", "x = 3\n"),
            ],
        );
        check(
            "matlabsession",
            "??? Error\n>> y\n",
            &[
                ("Generic.Traceback", "??? Error\n"),
                ("Generic.Prompt", ">> "),
                ("Name", "y"),
                ("Text.Whitespace", "\n"),
            ],
        );
    }

    #[test]
    fn psql_structure() {
        // Only the console-level tokens: the SQL inside depends on PsqlRegexLexer.
        let lexer = crate::lexer_by_name("psql").expect("psql");
        let text = "$ psql\nfoo=# SELECT 1;\nERROR:  bad\nfoo=# \\dt\nNOTICE: n\n";
        let toks: Vec<(TokenType, &str)> = lexer.tokenize(text).collect();
        assert_eq!(toks.iter().map(|t| t.1).collect::<String>(), text);
        let generic: Vec<(TokenType, &str)> =
            toks.into_iter().filter(|t| t.0.is_subtype_of(TokenType::GENERIC)).collect();
        let want = [
            (TokenType::GENERIC_PROMPT, "$ "),
            (TokenType::GENERIC_PROMPT, "foo=#"),
            (TokenType::GENERIC_STRONG, "ERROR:"),
            (TokenType::GENERIC_ERROR, "  bad\n"),
            (TokenType::GENERIC_PROMPT, "foo=#"),
            (TokenType::GENERIC_STRONG, "NOTICE:"),
            (TokenType::GENERIC_OUTPUT, " n\n"),
        ];
        assert_eq!(generic, want);
    }

    #[test]
    fn literate() {
        check(
            "lhs",
            "> main = x\ntext\n",
            &[
                ("Comment.Special", "> "),
                ("Name.Function", "main"),
                ("Text.Whitespace", " "),
                ("Operator.Word", "="),
                ("Text.Whitespace", " "),
                ("Name", "x"),
                ("Text.Whitespace", "\n"),
                ("Text", "text\n"),
            ],
        );
        check(
            "lhs",
            "\\begin{code}\nx = 1\n\\end{code}\n",
            &[
                ("Keyword", "\\begin"),
                ("Name.Builtin", "{"),
                ("Text", "code"),
                ("Name.Builtin", "}"),
                ("Text", "\n"),
                ("Name.Function", "x"),
                ("Text.Whitespace", " "),
                ("Operator.Word", "="),
                ("Text.Whitespace", " "),
                ("Literal.Number.Integer", "1"),
                ("Text.Whitespace", "\n"),
                ("Keyword", "\\end"),
                ("Name.Builtin", "{"),
                ("Text", "code"),
                ("Name.Builtin", "}"),
                ("Text", "\n"),
            ],
        );
        check(
            "lagda",
            "\\begin{code}\nx = 1\n\\end{code}\n",
            &[
                ("Keyword", "\\begin"),
                ("Name.Builtin", "{"),
                ("Text", "code"),
                ("Name.Builtin", "}"),
                ("Text", "\n"),
                ("Text", "x"),
                ("Text.Whitespace", " "),
                ("Operator.Word", "="),
                ("Text.Whitespace", " "),
                ("Literal.Number.Integer", "1"),
                ("Text.Whitespace", "\n"),
                ("Keyword", "\\end"),
                ("Name.Builtin", "{"),
                ("Text", "code"),
                ("Name.Builtin", "}"),
                ("Text", "\n"),
            ],
        );
        check("lidr", "> x\n", &[("Comment.Special", "> "), ("Text", "x"), ("Text.Whitespace", "\n")]);
        check("lcry", "> x\n", &[("Comment.Special", "> "), ("Name.Function", "x"), ("Text.Whitespace", "\n")]);
    }

    #[test]
    fn special() {
        check("text", "a b\n", &[("Text", "a b\n")]);
        check("output", "a b\n", &[("Generic.Output", "a b\n")]);
        let raw = crate::registry::by_class("RawTokenLexer").expect("raw");
        let text = "Token.Name\t'foo'\nbad\nToken.Foo.Bar\t\"x\"\nToken.Text\tb'x'\nToken.Text\t'a' 'b'\ntoken.x\t'a'\nToken.Name\t'\\x4'\n";
        let mut toks = Vec::new();
        raw.tokens_unprocessed(text, None, &mut toks);
        let types: Vec<TokenType> = toks.iter().map(|t| t.ty).collect();
        // Pygments: Name, Error, Token.Foo.Bar (unknown here, so Token), Error, Text, Error, Error.
        let e = TokenType::ERROR;
        assert_eq!(types, [TokenType::NAME, e, TokenType::TOKEN, e, TokenType::TEXT, e, e]);
        assert_eq!(toks.last().map(|t| t.end), Some(text.len()));
    }

    #[test]
    fn helpers() {
        assert_eq!(splitlines("a\r\nb\x0bc"), [(0, 3), (3, 5), (5, 6)]);
        assert_eq!(lines("a\nb").collect::<Vec<_>>(), [(0, 2)]);
        assert_eq!(char_prefix(">é\n", 2), 3);
        assert!(is_str_literal("'it\\'s' u\"x\""));
        assert!(!is_str_literal("'abc"));
        assert!(!is_str_literal("b'x'"));
    }
}
