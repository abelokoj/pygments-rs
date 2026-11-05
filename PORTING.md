# Porting notes

These notes cover the port of Pygments 2.19.2 (`pygments/lexer.py`, `token.py`, `lexers/*`,
`styles/*`, `util.py`, `modeline.py`) to Rust.

## How the port is split

| Pygments | Rust | How |
|---|---|---|
| `RegexLexer`, `ExtendedRegexLexer` (`get_tokens_unprocessed`, `_process_state`, `include`, `inherit`, `combined`, `default`, `words`, `#push`, `#pop[:n]`, negative pops) | `src/engine.rs` (`RegexLexer`, `Run`) | Engine port; rule tables converted to `data/lexers/*.json` |
| `bygroups`, `using(this / Other, state=)` | `Action::ByGroups`, `Action::Using`, `engine::{by_groups, using}` | Engine port |
| Rule callbacks | `src/hooks/callbacks_*` | Hand port, keyed by `module::qualname` |
| `get_tokens_unprocessed` overrides that remap tokens | `src/hooks/filters` (`FilterFn`) | Hand port |
| Lexers with a hand-written `get_tokens_unprocessed` | `src/hooks/{custom,consoles}` (`CustomFn`) | Hand port |
| `DelegatingLexer` | `hooks::delegate` | Engine port |
| `do_insertions` | `hooks::do_insertions` | Exact port |
| `_TokenType`, `Token.*`, `STANDARD_TYPES` | `TokenType` (`src/token.rs`, generated table) | Converted |
| `Lexer.get_tokens` preprocessing (BOM, newlines, `stripnl`, `stripall`, `tabsize`, `ensurenl`) | `Lexer::preprocess` | Port |
| `get_lexer_by_name` / `_for_filename` / `_for_mimetype`, `guess_lexer`, `fnmatch` | `src/registry.rs` | Port with the same rating and tie-break rules |
| `analyse_text`, `make_analysator`, `shebang_matches`, `doctype_matches`, `html_doctype_matches`, `looks_like_xml`, `modeline.get_filetype_from_buffer` | `src/analyse.rs`, `src/hooks/analysers` | Simple ones converted to an IR; the rest hand-ported |
| `Style` subclasses | `Style`, `data/styles.json` | Converted after Pygments resolves inheritance |

## Test mapping

| Pygments test | Rust test |
|---|---|
| `tests/snippets/**` (`conftest.LexerInlineTestItem`) | `tests/golden.rs::golden` |
| `tests/examplefiles/**` (`conftest.LexerSeparateTestItem`, no `Error` tokens) | `tests/golden.rs::golden` |
| `test_basic_api.py` (lexer metadata, every lexer loads and round-trips) | `tests/api.rs::every_lexer_loads_and_roundtrips`, `lookup_by_name_filename_mimetype` |
| `test_guess.py` (shebangs, modelines, XML) | `tests/api.rs::guessing` |
| `test_util.py` (`shebang_matches`, `doctype_matches`, `looks_like_xml`) | `src/analyse.rs` unit tests |
| `test_regexlexer.py`, `test_using_api.py` | engine behavior covered by the golden tests |

## Deliberate differences

- **Offsets are byte offsets into UTF-8.** Pygments uses code-point indices. Tokens are `&str`
  slices, so this is invisible unless you use `next_span()`.
- **Lone UTF-16 surrogates.** Pygments' Unicode category classes contain lone surrogates. A Rust
  `&str` can never contain one, so the converter removes them from patterns.
- **No lexer options.** Every lexer runs with its defaults.
- **Incremental resume (`LexState`, `tokenize_from`)** is new API. A full run yields exactly what a
  single Pygments run yields.

## Preserved quirks

These are intentional, so don't "fix" them:

- An unmatched `\n` resets the stack to `root` and emits `Text.Whitespace` (`Text` in extended
  lexers). Any other unmatched character emits one `Error` token.
- An empty match with no state change counts as "no match", as in Pygments.
- `bygroups` skips empty groups. A callback in a group gets a pseudo-match whose only group is the
  whole span.
- Filename ratings add +0.5 for globs without `*`, ties go to the highest class name, and
  `guess_lexer` returns at the first score of 1.0 in class-name order.
- `analyse_text` scores are clamped to `[0, 1]`, and an exception scores 0.

## Verification

Nothing is built or tested locally. By project rule, all building and testing runs in GitHub
Actions (`.github/workflows/ci.yml`): rustfmt, `clippy --all-targets -D warnings`, unit and API
tests on Linux and Windows, golden tests against Pygments' own expected outputs, and a check that
the converted data matches a fresh converter run. The expected outputs for the hand-ported hooks'
unit tests were taken from running Pygments 2.19.2.
