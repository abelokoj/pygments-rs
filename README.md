# pygments-rs

A pure-Rust syntax highlighter that ports [Pygments](https://pygments.org) 2.19.2, the way
[Chroma](https://github.com/alecthomas/chroma) did for Go: Pygments' lexer rule tables, token
types and styles are converted to data and embedded at compile time, and the Python pieces that
cannot be expressed as data are hand-ported to Rust. No Python at runtime.

- **All 597 Pygments lexers** (plus 12 internal helper lexers) are registered, with their names, aliases, filename globs, MIME types
  and `analyse_text` heuristics.
- **169 token types**, the full `Token.*` hierarchy.
- **49 styles**, including monokai, dracula, github-dark, solarized-dark/light, nord,
  gruvbox-dark/light, one-dark, material, vs and default.
- **All 8 token filters** (`codetagify`, `keywordcase`, `highlight`, `raiseonerror`, `whitespace`,
  `gobble`, `tokenmerge`, `symbols`) and **all 14 pure-text formatters** (`html`, `svg`, `latex`,
  `rtf`, `terminal`, `terminal256`, `terminal16m`, `bbcode`, `pango`, `irc`, `groff`, `raw`,
  `text`/`null`, `testcase`), plus the `pygmentize` CLI binary.
- Regexes run on [Oniguruma](https://github.com/kkos/oniguruma) in its Python-syntax mode with
  Python Unicode word/boundary and flag-scoping compatibility, so Pygments' patterns keep their
  exact meaning (lookbehind, `\Z`, `(?P<name>)`, inline flags).

## Usage

```rust
use pygments_rs::formatters::HtmlFormatter;
use pygments_rs::{
    guess_lexer, highlight, lexer_by_name, lexer_for_filename, style_by_name, TokenType,
};

let lexer = lexer_for_filename("main.rs", None).unwrap();
for (ty, text) in lexer.tokenize("fn main() {}\n") {
    let style = style_by_name("monokai").unwrap().style_for(ty);
    println!("{:<20} {text:?} {:?}", ty.to_string(), style.color);
}
let html = HtmlFormatter::new(style_by_name("monokai").unwrap());
let rendered = highlight("fn main() {}\n", lexer, &html);
assert!(rendered.contains("class=\"k\""));
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
| `get_filter_by_name(name, ...)` | `filter_by_name(name, ...)` / `apply_filters` |
| `get_formatter_by_name(alias, ...)` | `formatter_by_name(alias, ...)` / `formatter_for_filename` |
| `highlight(code, lexer, formatter)` | `highlight(code, lexer, &formatter)` |
| `pygmentize` CLI | `cargo run --bin pygmentize -- ...` |

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

Each lexer (597 public + 12 internal helpers = 609 total) falls into one of these groups:

- **405 lexers convert fully to data.** No Rust code is specific to them.
- **204 lexers need hand-ported pieces**, all in `src/hooks/`:
  - fully custom lexers (JSON, Delphi, Makefile, PythonConsole, the console/session lexers, the Literate lexers, ...)
  - rule callbacks (YAML, Ruby heredocs, XQuery, Email headers, Slash interpolation, Markdown code blocks, ...)
  - `get_tokens_unprocessed` overrides (C-family, Lua, PHP, ...)
  - `analyse_text` heuristics (`src/hooks/analysers/`) plus rule-IR heuristics converted automatically.

`target/catalog-report.txt` (from CI) lists any pattern Oniguruma rejects and any lexer that does
not round-trip its input.

The golden tests run every file in Pygments' `tests/snippets` and `tests/examplefiles` and compare
the token stream byte for byte with Pygments' expected output. CI publishes the pass rate per
lexer in the job summary (`target/golden-report.md`). Files that pass are pinned in
`tests/golden-baseline.txt`, and any regression fails the build.

### Known gaps

- Common preprocessing options (`stripnl`, `stripall`, `ensurenl`, `tabsize`) are supported via
  `LexOptions` and `Lexer::preprocess_with`; non-default language-specific lexer constructor options
  (such as C# `unicodelevel` or Email `highlight-X-header`) use Pygments defaults.
- Raster image formatters (`img`/`png`/`bmp`/`gif`/`jpg`, which require Python's Pillow/PIL) and
  ctags cross-reference linking in `HtmlFormatter` are out of scope.
- `tests/known-regex-errors.txt` lists any regex pattern Oniguruma cannot compile.

## Regenerating the data

The converter is a development tool. Its output is checked in, and CI verifies that it is up to date.

```bash
git clone --depth 1 --branch "$(cat PYGMENTS_VERSION)" https://github.com/pygments/pygments.git ../pygments
python -I tools/convert.py --pygments ../pygments
```

It writes `data/lexers/*.json`, `data/analyse.json`, `data/styles.json`, `data/report.json`
(what each lexer needs hand-ported), and `src/generated/{tokens,symbols,registry}.rs`.

## License

BSD-2-Clause, the same as Pygments. See [LICENSE](LICENSE).
