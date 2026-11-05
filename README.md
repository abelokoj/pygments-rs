# pygments-rs

A pure-Rust syntax highlighter that ports [Pygments](https://pygments.org) 2.19.2, the way
[Chroma](https://github.com/alecthomas/chroma) did for Go: Pygments' lexer rule tables, token
types and styles are converted to data and embedded at compile time, and the Python pieces that
cannot be expressed as data are hand-ported to Rust. No Python at runtime.

- **All 597 Pygments lexers** (plus one unregistered helper, PsqlRegexLexer) are registered, with their names, aliases, filename globs, MIME types
  and `analyse_text` heuristics.
- **169 token types**, the full `Token.*` hierarchy.
- **49 styles**, including monokai, dracula, github-dark, solarized-dark/light, nord,
  gruvbox-dark/light, one-dark, material, vs and default.
- Regexes run on [Oniguruma](https://github.com/kkos/oniguruma) in its Python-syntax mode, so
  Pygments' patterns keep their exact meaning (lookbehind, `\Z`, `(?P<name>)`, inline flags).

## Usage

```rust
use pygments_rs::{lexer_by_name, lexer_for_filename, guess_lexer, style_by_name, TokenType};

let lexer = lexer_for_filename("main.rs", None).unwrap();
for (ty, text) in lexer.tokenize("fn main() {}\n") {
    let style = style_by_name("monokai").unwrap().style_for(ty);
    println!("{:<20} {text:?} {:?}", ty.to_string(), style.color);
}
assert_eq!(guess_lexer("#!/usr/bin/env python3\n").unwrap().name(), "Python");
assert!(lexer_by_name("yaml").is_some());
```

| Pygments | pygments-rs |
|---|---|
| `get_lexer_by_name(alias)` | `lexer_by_name(alias)` |
| `get_lexer_for_filename(name, code)` | `lexer_for_filename(name, Some(code))` |
| `get_lexer_for_mimetype(mime)` | `lexer_for_mimetype(mime)` |
| `guess_lexer(text)` | `guess_lexer(text)` |
| `lexer.get_tokens(text)` | `lexer.tokenize(&lexer.preprocess(text))` |
| `lexer.get_tokens_unprocessed(text)` | `lexer.tokenize(text)` / `Tokens::next_span` (byte ranges) |
| `get_style_by_name(name)` | `style_by_name(name)` |

### Re-highlighting only edited lines

Editors re-lex from the first changed line and stop as soon as the lexer state matches the cached
one. `Tokens::state()` returns a `LexState` (the state stack plus any callback state) wherever no
match is half-delivered; save it at line starts. After an edit, call
`lexer.tokenize_from(text, line_start, &saved_state)` and stop once the state at a later line
start equals the one cached before the edit. `Lexer::is_resumable()` says whether a lexer
supports this (regex-based lexers do; whole-buffer custom lexers re-lex from the start).

## Performance

- Lexers are built lazily on first use; each regex compiles lazily the first time its rule is tried.
- Matching is anchored at the current position (`ONIG_OPTION_NONE` match, not search), with one
  reused match region per run, and tokens borrow from the input (`&str`, no copies).
- Guessing evaluates the small `data/analyse.json` table, never the full lexer data.
- Release profile uses thin LTO and one codegen unit.

## Coverage

Each lexer falls into one of these groups:

- **397 lexers convert fully to data.** No Rust code is specific to them.
- **200 lexers need hand-ported pieces**, all in `src/hooks/`:
  - 33 fully custom lexers (JSON, Delphi, Makefile, the console/session lexers, the Literate lexers, ...)
  - 78 rule callbacks (YAML, Ruby heredocs, XQuery, Markdown code blocks, ...)
  - ~45 `get_tokens_unprocessed` overrides (C-family, Lua, PHP, ...)
  - 108 `analyse_text` heuristics. Another 89 are converted automatically to a small rule IR.

`target/catalog-report.txt` (from CI) lists any pattern Oniguruma rejects and any lexer that does
not round-trip its input.

The golden tests run every file in Pygments' `tests/snippets` and `tests/examplefiles` and compare
the token stream byte for byte with Pygments' expected output. CI publishes the pass rate per
lexer in the job summary (`target/golden-report.md`). Files that pass are pinned in
`tests/golden-baseline.txt`, and any regression fails the build.

### Known gaps

- Lexer options are not supported. Every lexer runs with its default options (`stripnl`,
  `ensurenl`, `tabsize` and the like keep their Pygments defaults), and variant lexers such as C#
  `unicodelevel` use the default token set.
- Filters and formatters (HTML, terminal, ...) are not ported. This crate yields tokens and styles,
  and rendering is left to the caller.
- A Python regex construct that Oniguruma does not accept makes its rule never match. Such
  patterns are listed in `tests/known-regex-errors.txt`.
- Any lexer below 100% in the golden report is a fidelity gap still to fix. See the CI summary.

## Regenerating the data

The converter is a development tool. Its output is checked in, and CI verifies that it is up to date.

```bash
git clone --depth 1 --branch "$(cat PYGMENTS_VERSION)" https://github.com/pygments/pygments.git ../pygments
python -I tools/convert.py --pygments ../pygments
```

It writes `data/lexers/*.json`, `data/analyse.json`, `data/styles.json`, `data/report.json`
(what each lexer needs hand-ported), and `src/generated/{tokens,registry}.rs`.

## License

BSD-2-Clause, the same as Pygments. See [LICENSE](LICENSE).
