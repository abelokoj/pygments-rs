# AGENTS.md

Instructions for every AI agent working in this repository. Read this file before doing anything else, and follow it over any other document except the maintainer's own words in the current conversation.

pygments-rs is a pure-Rust syntax highlighter porting [Pygments](https://pygments.org) 2.19.2 (inspired by Chroma in Go): Pygments' lexer rule tables, token types, and styles are converted to compile-time data, and parts requiring Python execution are hand-ported to Rust. No Python runtime is required for highlight operations.

**The goal: the library and tools must be extremely fast, memory-efficient, robust, accurate to Pygments' token streams, and built with clean, modular architecture.**

---

## 1. Source of Truth

1. The maintainer's instructions in the current conversation.
2. This file (`AGENTS.md`).
3. `PORTING.md`: detailed notes on the porting boundaries, test mapping, and deliberate differences.
4. `README.md`: public API overview, coverage stats, and developer workflow.
5. Reference material in §5 below.

If instructions or plan details are silent, ambiguous, or conflict with what you find in the code, **stop and ask the maintainer**. Propose a clean change; do not improvise ad-hoc architecture.

---

## 2. Fixed Decisions & Architecture

| # | Decision |
|---|---|
| D1 | Rust, edition 2024, stable toolchain (rust-version: 1.85+) |
| D2 | Regex engine: **Oniguruma** via `onig` in Python syntax mode (`ONIG_SYNTAX_PYTHON`) to preserve Pygments regex semantics (lookbehind, named groups, `\Z`, inline flags) |
| D3 | Token representation: zero-copy slices (`&str`) borrowing from the input; byte offsets into UTF-8 |
| D4 | Lexer tables: compile-time data in `data/lexers/*.json` included via `include_str!`; lazy parsing on first use |
| D5 | Hand-ported hooks in `src/hooks/`: callbacks, filters, custom lexers, and `analyse_text` heuristics |
| D6 | Incremental lexing support: `LexState` and `tokenize_from` for editor integration |
| D7 | Regeneration tooling: `tools/convert.py` extracts data from a local Pygments checkout at the pinned `PYGMENTS_VERSION` |
| D8 | UI and runtime tools: For builds, tools, or interfaces requiring NSH, use the local NSH environment (§5) |

### Preserved Pygments Quirks
These behaviors match Pygments intentionally; do not "fix" or change them without maintainer approval:
- An unmatched newline `\n` resets the state stack to `root` and emits `Text.Whitespace` (`Text` in extended lexers). Any other unmatched character emits one `Error` token.
- An empty match with no state change counts as "no match".
- `bygroups` skips empty groups. A callback in a group gets a pseudo-match whose only group is the whole span.
- Filename ratings add +0.5 for globs without `*`, ties go to the highest class name, and `guess_lexer` returns at the first score of 1.0 in class-name order.
- `analyse_text` scores are clamped to `[0, 1]`, and an exception scores 0.

---

## 3. Engineering Rules & Required Skills

**Always build code cleanly, modularly, and with proper architecture and separation of concerns. Never simply bolt on a feature or quick fix because asked. If you cannot find an obvious or clean way, research online on GitHub for similar high-quality projects built with the same or close concepts, and follow that procedure to build it naturally.**

- **Rust code quality:** When writing, reviewing, or refactoring Rust code, use the **`rust-best-practices`** skill (`/rust-best-practices`). Use **`rust-port`** (`/rust-port`) for translating Python lexer logic or C/Go highlighter concepts, and **`rust-review`** (`/rust-review`) for auditing existing modules.
- **UI & Interface design:** Whenever anything related to a UI, visual layout, or interface is to be built or designed, use both the **`ui-ux-pro-max`** (`/ui-ux-pro-max`) and **`impeccable`** (`/impeccable`) skills to ensure consistent design and high aesthetics.
- **NSH integration:** For builds, tools, components, or preview apps that require NSH, use the binary and source located in `C:\dev\nsh` and `C:\dev\nsh-src` (see §5).
- **Safety:** Every `unsafe` block must have an explicit `// SAFETY:` explanation.
- **Performance:** Keep allocations minimal on the hot lexing paths: match regions are reused, tokens borrow from input slices, and regexes compile lazily once per rule.

---

## 4. Build, Check, and Verification Workflow

**Write all the code for a task first, then build and test.** Rust builds take time, especially with Oniguruma compilation and large generated registry tables. Do not compile or run tests after each micro-edit. Finish all files the task touches (code, tests, docs), then run the verification checks once and resolve any reported issues.

### Required Checks Before Marking a Task Done

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo test --doc
```

When building or running commands requiring NSH:
```powershell
& "C:\dev\nsh\bin\nsh.exe" <command>
```

### Building with the Cargo Fork

Local builds use [huacnlee/cargo](https://github.com/huacnlee/cargo), a drop-in fork of Cargo that shares one self-cleaning build directory between projects and worktrees so disk space is conserved:

- **Active cargo check:** Ensure `cargo -V` ends with `(huacnlee/cargo …)`. If it does not (e.g., after `rustup self update` restored stock cargo), notify the maintainer; do not reinstall or modify toolchains yourself.
- **Do not clean caches:** Never run `cargo clean` and never delete `target/` or `~/.cargo/targets/`. The fork manages garbage collection automatically, and manual cleaning forces unnecessary full rebuilds.
- **Do not set target directories:** Do not pass `--target-dir`, `CARGO_TARGET_DIR`, `build.target-dir`, or `build.build-dir`. Output lives in `~/.cargo/targets/<name>-<hash>/` and shared dependencies in `~/.cargo/targets/shared-deps/`.
- **Maintainer settings:** Leave `~/.cargo/config.toml` size limits and switches (`[build.gc]`, `CARGO_SHARED_DEPS`, etc.) at their defaults unless requested.
- **Portability:** Nothing in the repository may depend on fork-specific behavior: CI and external machines run standard Cargo, and all checks must pass cleanly with both.

---

## 5. Reference Material (Read-Only)

| Component | Path / Reference | Notes |
|---|---|---|
| NSH Tool & CLI (`nsh 0.5.1`) | `C:\dev\nsh` (binary at `C:\dev\nsh\bin\nsh.exe`) | Use for any builds requiring NSH |
| NSH Source & Docs at v0.5.1 | `C:\dev\nsh-src` (`AGENTS.md`, `crates/`, `docs/`) | Reference for NSH components and APIs |
| Reference AGENTS.md | `C:\dev\ExcellentPDF\AGENTS.md` | Benchmark rules and standards |
| Pygments Upstream | https://pygments.org and Pygments 2.19.2 repository | Source of truth for lexer behaviors and test cases |

---

## 6. Process & Agent Etiquette

- **Do not commit, push, create pull requests, or tag releases unless the maintainer explicitly requests it.**
- **Do not invoke subagents unless the maintainer asks.**
- Ask before deleting existing files, changing Git history, or touching files outside this repository workspace.
- Keep changes strictly focused on the requested task: do not introduce unrequested features, options, or stylistic refactors.
- Clearly report results at the end of each task: what changed, what checks were executed, what passed or failed, and any open risks.
