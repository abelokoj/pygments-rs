# TODO

## Update to Pygments 2.21.0

The port tracks Pygments 2.19.2, where all 1407 golden files pass. Upstream has since
released 2.20.0 (2026-03-29) and 2.21.0 (2026-08-17).

What changed between 2.19.2 and 2.21.0 (checked 2026-11-21):

| | 2.19.2 (current) | 2.21.0 |
|---|---|---|
| Lexers | 597 | 602 |
| Golden test files | 1407 | 1508 |
| Styles | 49 | 50 |

- New lexers: BitBakeLexer, CELLexer, CaddyfileLexer, PureScriptLexer, RellLexer, RocqLexer.
- Removed: CoqLexer (apparently renamed to RocqLexer).
- Lexer metadata changed: CppLexer, FennelLexer, JsonLexer, MathematicaLexer, MesonLexer,
  PythonLexer, XmlLexer, YamlLexer.
- New style: `night_owl`.
- 264 files differ under `pygments/lexers/`.

Steps:

1. Read the 2.20.0 and 2.21.0 changelogs (`CHANGES` in the Pygments repo).
2. Set `PYGMENTS_VERSION` to `2.21.0`. Then run
   `python -I tools/convert.py --pygments <2.21.0 checkout>`.
3. Read the new `data/report.json` for lexers that now need hand-ported pieces. Diff each
   hand-ported lexer's Python source between the two tags (JSON, YAML, Python and C++ first,
   since their metadata changed), and update the matching code in `src/hooks/`.
4. Let CI run the golden suite against the 2.21.0 test files. Fix failures until every file
   passes, then refresh `tests/golden-baseline.txt` from `target/golden-passing.txt`.
5. Update the counts in `README.md` and `PORTING.md`.
