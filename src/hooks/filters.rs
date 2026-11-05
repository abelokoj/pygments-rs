//! `get_tokens_unprocessed` overrides that rewrite a lexer's token stream
//! (keyword tables applied to `Name` tokens, Lua dotted names, Shen
//! declarations, Inform 6 `objectloop`, TADS 3 `#if 0`, Modula-2 dialects).
//!
//! A subclass override that calls its parent's override is one filter that
//! applies both mappings in the same order. Filters that keep state store it
//! in `Extra`; a filter that needs end-of-input (Inform 6, Shen) detects it as
//! the token that ends at `text.len()`.

use std::borrow::Cow;
use std::collections::{HashSet, VecDeque};
use std::sync::{LazyLock, OnceLock};

use onig::{Regex, SearchOptions};
use serde_json::Value;

use super::{Extra, FilterFn, Module};
use crate::engine::{Tok, compile};
use crate::lexer::LexerImpl;
use crate::token::TokenType as T;

pub(crate) static MODULE: Module = Module { callbacks: &[], filters: FILTERS, custom: &[], analyse: &[] };

static FILTERS: &[(&str, FilterFn)] = &[
    // CFamilyLexer
    ("CLexer", c_family),
    ("CharmciLexer", c_family),
    ("CppLexer", c_family),
    ("ECLexer", c_family),
    ("MqlLexer", c_family),
    ("NesCLexer", c_family),
    ("OmgIdlLexer", c_family),
    ("PikeLexer", c_family),
    ("PromelaLexer", c_family),
    ("SwigLexer", c_family),
    ("ArduinoLexer", arduino),
    ("CudaLexer", cuda),
    ("FreeFemLexer", freefem),
    // GeneratedObjectiveCVariant (on top of CFamilyLexer)
    ("ObjectiveCLexer", objective_c),
    ("ObjectiveCppLexer", objective_c),
    ("LogosLexer", objective_c),
    ("SwiftLexer", swift),
    ("AspectJLexer", aspectj),
    ("AsymptoteLexer", asymptote),
    ("CommonLispLexer", common_lisp),
    ("CryptolLexer", cryptol),
    ("DylanLexer", dylan),
    ("ElixirLexer", elixir),
    ("EmacsLispLexer", emacs_lisp),
    ("Inform6Lexer", inform6),
    ("LassoLexer", lasso),
    ("LilyPondLexer", lilypond),
    ("LuaLexer", lua),
    ("LuauLexer", luau),
    ("Modula2Lexer", modula2),
    ("MoonScriptLexer", moonscript),
    ("NumPyLexer", numpy),
    ("PhpLexer", php),
    ("SchemeLexer", scheme),
    ("ShenLexer", shen),
    ("SlurmBashLexer", slurm_bash),
    ("SourcePawnLexer", sourcepawn),
    ("Tads3Lexer", tads3),
    ("VimLexer", vim),
    ("XsltLexer", xslt),
];

// -- helpers ------------------------------------------------------------------

fn val<'t>(text: &'t str, tok: &Tok) -> &'t str {
    text.get(tok.start..tok.end).unwrap_or("")
}

fn has(lx: &LexerImpl, set: &str, v: &str) -> bool {
    lx.word_set(set).contains(v)
}

/// The type of the first `(const set, type)` pair whose set contains `v`.
fn first(lx: &LexerImpl, v: &str, table: &[(&str, T)]) -> Option<T> {
    table.iter().find(|(set, _)| has(lx, set, v)).map(|&(_, ty)| ty)
}

/// Python's `str.lower()` without allocating for already-lowercase ASCII.
fn lower(s: &str) -> Cow<'_, str> {
    if s.bytes().all(|b| b.is_ascii() && !b.is_ascii_uppercase()) {
        Cow::Borrowed(s)
    } else {
        Cow::Owned(s.to_lowercase())
    }
}

/// A filter that maps each token's type through the given functions in order.
macro_rules! map_filter {
    ($name:ident: $($f:ident),+) => {
        fn $name(lx: &'static LexerImpl, text: &str, tok: Tok, _: &mut Extra, out: &mut VecDeque<Tok>) {
            let v = val(text, &tok);
            let mut ty = tok.ty;
            $(ty = $f(lx, v, ty);)+
            out.push_back(Tok { ty, ..tok });
        }
    };
}

map_filter!(c_family: cfamily_map);
map_filter!(arduino: cfamily_map, arduino_map);
map_filter!(cuda: cfamily_map, cuda_map);
map_filter!(freefem: cfamily_map, freefem_map);
map_filter!(objective_c: cfamily_map, cocoa_map);
map_filter!(swift: cocoa_map);
map_filter!(asymptote: asymptote_map);
map_filter!(common_lisp: common_lisp_map);
map_filter!(cryptol: cryptol_map);
map_filter!(dylan: dylan_map);
map_filter!(elixir: elixir_map);
map_filter!(emacs_lisp: emacs_lisp_map);
map_filter!(lasso: lasso_map);
map_filter!(scheme: scheme_map);
map_filter!(lilypond: scheme_map, lilypond_map);
map_filter!(numpy: numpy_map);
map_filter!(php: php_map);
map_filter!(slurm_bash: slurm_map);
map_filter!(sourcepawn: sourcepawn_map);
map_filter!(vim: vim_map);
map_filter!(xslt: xslt_map);

// -- one-to-one type maps -------------------------------------------------------

fn cfamily_map(lx: &LexerImpl, v: &str, ty: T) -> T {
    // The four highlighting options default to true.
    let types = ["stdlib_types", "c99_types", "c11_atomic_types", "linux_types"];
    if ty == T::NAME && types.iter().any(|s| has(lx, s, v)) { T::KEYWORD_TYPE } else { ty }
}

fn arduino_map(lx: &LexerImpl, v: &str, ty: T) -> T {
    first(
        lx,
        v,
        &[
            ("structure", T::NAME_BUILTIN),
            ("operators", T::OPERATOR),
            ("variables", T::KEYWORD_RESERVED),
            ("suppress_highlight", T::NAME),
            ("functions", T::NAME_FUNCTION),
        ],
    )
    .unwrap_or(ty)
}

fn cuda_map(lx: &LexerImpl, v: &str, ty: T) -> T {
    if ty != T::NAME {
        return ty;
    }
    first(
        lx,
        v,
        &[
            ("variable_qualifiers", T::KEYWORD_TYPE),
            ("vector_types", T::KEYWORD_TYPE),
            ("variables", T::NAME_BUILTIN),
            ("execution_confs", T::KEYWORD_PSEUDO),
            ("function_qualifiers", T::KEYWORD_RESERVED),
            ("functions", T::NAME_FUNCTION),
        ],
    )
    .unwrap_or(ty)
}

fn freefem_map(lx: &LexerImpl, v: &str, ty: T) -> T {
    first(
        lx,
        v,
        &[
            ("operators", T::OPERATOR),
            ("types", T::KEYWORD_TYPE),
            ("fespaces", T::NAME_CLASS),
            ("preprocessor", T::COMMENT_PREPROC),
            ("keywords", T::KEYWORD_RESERVED),
            ("functions", T::NAME_FUNCTION),
            ("parameters", T::KEYWORD_PSEUDO),
            ("suppress_highlight", T::NAME),
        ],
    )
    .unwrap_or(ty)
}

fn cocoa_map(lx: &LexerImpl, v: &str, ty: T) -> T {
    let sets = ["COCOA_INTERFACES", "COCOA_PROTOCOLS", "COCOA_PRIMITIVES"];
    if (ty == T::NAME || ty == T::NAME_CLASS) && sets.iter().any(|s| has(lx, s, v)) {
        T::NAME_BUILTIN_PSEUDO
    } else {
        ty
    }
}

fn asymptote_map(lx: &LexerImpl, v: &str, ty: T) -> T {
    if ty != T::NAME {
        return ty;
    }
    first(lx, v, &[("ASYFUNCNAME", T::NAME_FUNCTION), ("ASYVARNAME", T::NAME_VARIABLE)]).unwrap_or(ty)
}

fn common_lisp_map(lx: &LexerImpl, v: &str, ty: T) -> T {
    if ty != T::NAME_VARIABLE {
        return ty;
    }
    first(
        lx,
        v,
        &[
            ("builtin_function", T::NAME_BUILTIN),
            ("special_forms", T::KEYWORD),
            ("macros", T::NAME_BUILTIN),
            ("lambda_list_keywords", T::KEYWORD),
            ("declarations", T::KEYWORD),
            ("builtin_types", T::KEYWORD_TYPE),
            ("builtin_classes", T::NAME_CLASS),
        ],
    )
    .unwrap_or(ty)
}

fn cryptol_map(lx: &LexerImpl, v: &str, ty: T) -> T {
    if ty == T::NAME && has(lx, "EXTRA_KEYWORDS", v) { T::NAME_BUILTIN } else { ty }
}

fn dylan_map(lx: &LexerImpl, v: &str, ty: T) -> T {
    if ty != T::NAME {
        return ty;
    }
    let v = lower(v);
    first(
        lx,
        &v,
        &[
            ("builtins", T::NAME_BUILTIN),
            ("keywords", T::KEYWORD),
            ("functions", T::NAME_BUILTIN),
            ("operators", T::OPERATOR),
        ],
    )
    .unwrap_or(ty)
}

fn elixir_map(lx: &LexerImpl, v: &str, ty: T) -> T {
    if ty != T::NAME {
        return ty;
    }
    first(
        lx,
        v,
        &[
            ("KEYWORD", T::KEYWORD),
            ("KEYWORD_OPERATOR", T::OPERATOR_WORD),
            ("BUILTIN", T::KEYWORD),
            ("BUILTIN_DECLARATION", T::KEYWORD_DECLARATION),
            ("BUILTIN_NAMESPACE", T::KEYWORD_NAMESPACE),
            ("CONSTANT", T::NAME_CONSTANT),
            ("PSEUDO_VAR", T::NAME_BUILTIN_PSEUDO),
        ],
    )
    .unwrap_or(ty)
}

fn emacs_lisp_map(lx: &LexerImpl, v: &str, ty: T) -> T {
    if ty != T::NAME_VARIABLE {
        return ty;
    }
    first(
        lx,
        v,
        &[
            ("builtin_function", T::NAME_FUNCTION),
            ("special_forms", T::KEYWORD),
            ("error_keywords", T::NAME_EXCEPTION),
            ("builtin_function_highlighted", T::NAME_BUILTIN),
            ("macros", T::NAME_BUILTIN),
            ("lambda_list_keywords", T::KEYWORD_PSEUDO),
        ],
    )
    .unwrap_or(ty)
}

fn lasso_map(lx: &LexerImpl, v: &str, ty: T) -> T {
    let builtin = (ty == T::NAME_OTHER && has(lx, "_builtins", &lower(v)))
        || (ty == T::NAME_OTHER_MEMBER && has(lx, "_members", lower(v).trim_end_matches('=')));
    if builtin { T::NAME_BUILTIN } else { ty }
}

fn scheme_map(lx: &LexerImpl, v: &str, ty: T) -> T {
    if ty != T::NAME_FUNCTION && ty != T::NAME_VARIABLE {
        return ty;
    }
    first(lx, v, &[("scheme_keywords", T::KEYWORD), ("scheme_builtins", T::NAME_BUILTIN)]).unwrap_or(ty)
}

fn lilypond_map(lx: &LexerImpl, v: &str, ty: T) -> T {
    // These types are created on first use in Python, so the generated table
    // may lack them; fall back to the parent until convert.py adds them.
    static SCHEME_FUNCTION: LazyLock<T> =
        LazyLock::new(|| T::from_name("Name.Builtin.SchemeFunction").unwrap_or(T::NAME_BUILTIN));
    static SCHEME_BUILTIN: LazyLock<T> =
        LazyLock::new(|| T::from_name("Name.Builtin.SchemeBuiltin").unwrap_or(T::NAME_BUILTIN));
    if ty == T::NAME_FUNCTION || ty == T::NAME_VARIABLE {
        if has(lx, "scheme_functions", v) { *SCHEME_FUNCTION } else { ty }
    } else if ty == T::NAME_BUILTIN {
        *SCHEME_BUILTIN
    } else {
        ty
    }
}

fn numpy_map(lx: &LexerImpl, v: &str, ty: T) -> T {
    if ty == T::NAME && has(lx, "EXTRA_KEYWORDS", v) { T::KEYWORD_PSEUDO } else { ty }
}

fn php_map(lx: &LexerImpl, v: &str, ty: T) -> T {
    if ty == T::NAME_OTHER && has(lx, "_functions", v) { T::NAME_BUILTIN } else { ty }
}

fn slurm_map(lx: &LexerImpl, v: &str, ty: T) -> T {
    if ty == T::TEXT && has(lx, "EXTRA_KEYWORDS", v) {
        T::NAME_BUILTIN
    } else if ty == T::COMMENT_SINGLE && v.contains("SBATCH") {
        T::KEYWORD_PSEUDO
    } else {
        ty
    }
}

fn sourcepawn_map(lx: &LexerImpl, v: &str, ty: T) -> T {
    if ty != T::NAME {
        return ty;
    }
    first(lx, v, &[("SM_TYPES", T::KEYWORD_TYPE), ("_functions", T::NAME_BUILTIN)]).unwrap_or(ty)
}

/// `VimLexer.is_in`: `w` abbreviates an entry `[shortest, full]` of the
/// sorted list `key`.
fn vim_is_in(lx: &LexerImpl, key: &str, w: &str) -> bool {
    fn pair(v: &Value) -> (&str, &str) {
        (v.get(0).and_then(Value::as_str).unwrap_or(""), v.get(1).and_then(Value::as_str).unwrap_or(""))
    }
    let Some(map) = lx.konst(key).and_then(Value::as_array) else { return false };
    // bisect(mapping, (w,)): entries whose abbreviation sorts before `w`.
    let p = map.partition_point(|e| pair(e).0 < w);
    let ok = |i: usize| {
        let (abbr, full) = pair(&map[i]);
        w.starts_with(abbr) && full.starts_with(w)
    };
    (p > 0 && ok(p - 1)) || (p < map.len() && ok(p))
}

fn vim_map(lx: &LexerImpl, v: &str, ty: T) -> T {
    if ty != T::NAME_OTHER {
        ty
    } else if vim_is_in(lx, "_cmd", v) {
        T::KEYWORD
    } else if vim_is_in(lx, "_opt", v) || vim_is_in(lx, "_aut", v) {
        T::NAME_BUILTIN
    } else {
        T::TEXT
    }
}

fn xslt_map(lx: &LexerImpl, v: &str, ty: T) -> T {
    // re.match('</?xsl:([^>]*)/?>?', value).group(1)
    let name = v.strip_prefix('<').map(|r| r.strip_prefix('/').unwrap_or(r)).and_then(|r| r.strip_prefix("xsl:"));
    match name {
        Some(r) if ty == T::NAME_TAG && has(lx, "EXTRA_KEYWORDS", &r[..r.find('>').unwrap_or(r.len())]) => T::KEYWORD,
        _ => ty,
    }
}

// -- one-to-many filters --------------------------------------------------------

fn aspectj(lx: &'static LexerImpl, text: &str, tok: Tok, _: &mut Extra, out: &mut VecDeque<Tok>) {
    let v = val(text, &tok);
    if tok.ty == T::NAME_LABEL && has(lx, "aj_inter_type", v) {
        // value[:-1] as Keyword, the trailing ':' as Operator.
        let cut = tok.start + v.char_indices().last().map_or(0, |(i, _)| i);
        out.push_back(Tok::new(T::KEYWORD, tok.start, cut));
        out.push_back(Tok::new(T::OPERATOR, cut, tok.end));
        return;
    }
    let keyword = (tok.ty == T::NAME && has(lx, "aj_keywords", v))
        || (tok.ty == T::NAME_DECORATOR && has(lx, "aj_inter_type_annotation", v));
    out.push_back(if keyword { Tok { ty: T::KEYWORD, ..tok } } else { tok });
}

fn lua(lx: &'static LexerImpl, text: &str, tok: Tok, _: &mut Extra, out: &mut VecDeque<Tok>) {
    let v = val(text, &tok);
    if tok.ty != T::NAME_BUILTIN || has(lx, "_functions", v) {
        out.push_back(tok);
        return;
    }
    match v.find('.') {
        Some(d) => {
            let d = tok.start + d;
            out.push_back(Tok::new(T::NAME, tok.start, d));
            out.push_back(Tok::new(T::PUNCTUATION, d, d + 1));
            out.push_back(Tok::new(T::NAME, d + 1, tok.end));
        }
        None => out.push_back(Tok { ty: T::NAME, ..tok }),
    }
}

fn moonscript(lx: &'static LexerImpl, text: &str, tok: Tok, extra: &mut Extra, out: &mut VecDeque<Tok>) {
    let from = out.len();
    lua(lx, text, tok, extra, out);
    for t in out.range_mut(from..) {
        if t.ty == T::PUNCTUATION && text.get(t.start..t.end) == Some(".") {
            t.ty = T::OPERATOR;
        }
    }
}

fn luau(lx: &'static LexerImpl, text: &str, tok: Tok, _: &mut Extra, out: &mut VecDeque<Tok>) {
    if tok.ty != T::NAME && tok.ty != T::NAME_OTHER {
        out.push_back(tok);
        return;
    }
    let v = val(text, &tok);
    // Longest dotted prefix that is a builtin; the rest are variables/fields.
    let mut rest = 0;
    for end in std::iter::once(v.len()).chain(v.rmatch_indices('.').map(|(i, _)| i)) {
        if has(lx, "_builtins", &v[..end]) {
            out.push_back(Tok::new(T::NAME_BUILTIN, tok.start, tok.start + end));
            if end < v.len() {
                out.push_back(Tok::new(T::PUNCTUATION, tok.start + end, tok.start + end + 1));
                rest = end + 1;
            } else {
                rest = end;
            }
            break;
        }
    }
    if rest >= v.len() && rest > 0 {
        return;
    }
    let mut pos = tok.start + rest;
    let mut parts = v[rest..].split('.').peekable();
    while let Some(part) = parts.next() {
        let end = pos + part.len();
        if parts.peek().is_some() {
            out.push_back(Tok::new(T::NAME_VARIABLE, pos, end));
            out.push_back(Tok::new(T::PUNCTUATION, end, end + 1));
            pos = end + 1;
        } else {
            let ty = if tok.ty == T::NAME { T::NAME_VARIABLE } else { T::NAME_FUNCTION };
            out.push_back(Tok::new(ty, pos, end));
        }
    }
}

// -- Inform 6: `in` inside objectloop --------------------------------------------

#[derive(Clone, Debug, PartialEq)]
struct Inform6State {
    queue: Vec<Tok>,
    count: i32,
    previous: Option<T>,
}

impl Default for Inform6State {
    fn default() -> Inform6State {
        Inform6State { queue: Vec::new(), count: -1, previous: None }
    }
}

fn inform6(_: &'static LexerImpl, text: &str, tok: Tok, extra: &mut Extra, out: &mut VecDeque<Tok>) {
    let st = extra.get::<Inform6State>();
    let relevant = !tok.ty.is_subtype_of(T::COMMENT) && !tok.ty.is_subtype_of(T::TEXT);
    if st.previous == Some(T::NAME_VARIABLE) && val(text, &tok) == "in" {
        // Python replaces the queue here, dropping anything still queued.
        st.queue.clear();
        st.queue.push(tok);
        st.count = 2;
    } else if st.count > 0 {
        if relevant {
            st.count -= 1;
        }
        st.queue.push(tok);
    } else {
        if st.count == 0 {
            if st.queue.last().is_some_and(|t| val(text, t) == ")") {
                st.queue[0].ty = T::KEYWORD;
            }
            out.extend(st.queue.drain(..));
            st.count = -1;
        }
        out.push_back(tok);
    }
    if relevant {
        st.previous = Some(tok.ty);
    }
    if tok.end >= text.len() {
        out.extend(st.queue.drain(..));
    }
}

// -- TADS 3: code inside `#if 0` / `#if nil` is a comment -------------------------

fn tads3(_: &'static LexerImpl, text: &str, tok: Tok, extra: &mut Extra, out: &mut VecDeque<Tok>) {
    const WS: &str = r"(?:\\\n|[^\S\n]|(?://(?:[^\\\n]|\\+[\w\W])*$)|(?:/\*(?:[^*]|\*(?!/))*\*/))";
    fn pp(tail: &str) -> Regex {
        compile(&format!("^{WS}*#{WS}*{tail}"), "").expect("valid TADS 3 preprocessor pattern")
    }
    static IF_FALSE: LazyLock<Regex> = LazyLock::new(|| pp(&format!(r"if{WS}+(0|nil){WS}*$\n?")));
    static ELSE: LazyLock<Regex> = LazyLock::new(|| pp(r"el(if|se)\b"));
    static IF: LazyLock<Regex> = LazyLock::new(|| pp("if"));
    static ENDIF: LazyLock<Regex> = LazyLock::new(|| pp(r"endif\b"));
    fn hit(re: &Regex, v: &str) -> bool {
        re.match_with_options(v, 0, SearchOptions::SEARCH_OPTION_NONE, None).is_some()
    }

    #[derive(Clone, Debug, Default, PartialEq)]
    struct Level(i32);
    let level = &mut extra.get::<Level>().0;
    let v = val(text, &tok);
    let mut ty = tok.ty;
    if *level == 0 {
        if ty == T::COMMENT_PREPROC && hit(&IF_FALSE, v) {
            *level = 1;
        }
    } else if ty == T::COMMENT_PREPROC {
        if *level == 1 && hit(&ELSE, v) {
            *level = 0;
        } else if hit(&IF, v) {
            *level += 1;
        } else if hit(&ENDIF, v) {
            *level -= 1;
        }
    } else {
        ty = T::COMMENT;
    }
    out.push_back(Tok { ty, ..tok });
}

// -- Modula-2: dialect tags and dialect-specific errors ------------------------------

const M2_DIALECTS: [&str; 9] =
    ["unknown", "m2pim", "m2iso", "m2r10", "objm2", "m2iso+aglet", "m2pim+gm2", "m2iso+p1", "m2iso+xds"];

/// The word sets `Modula2Lexer.set_dialect` computes for one dialect.
struct M2Sets {
    reject: HashSet<Box<str>>,
    reserved: HashSet<Box<str>>,
    builtins: HashSet<Box<str>>,
    pseudo: HashSet<Box<str>>,
    adts: HashSet<Box<str>>,
    modules: HashSet<Box<str>>,
    types: HashSet<Box<str>>,
    procedures: HashSet<Box<str>>,
    variables: HashSet<Box<str>>,
    constants: HashSet<Box<str>>,
}

impl M2Sets {
    fn build(lx: &LexerImpl, dialect: &str) -> M2Sets {
        let union = |db: &str, minus: Option<&HashSet<Box<str>>>| -> HashSet<Box<str>> {
            let lists = lx.konst(db).and_then(|d| d.get(dialect)).and_then(Value::as_array);
            lists
                .into_iter()
                .flatten()
                .filter_map(Value::as_array)
                .flatten()
                .filter_map(Value::as_str)
                .filter(|w| minus.is_none_or(|m| !m.contains(*w)))
                .map(Box::from)
                .collect()
        };
        let reserved = union("reserved_words_db", None);
        let builtins = union("builtins_db", Some(&reserved));
        M2Sets {
            reject: union("lexemes_to_reject_db", None),
            pseudo: union("pseudo_builtins_db", Some(&reserved)),
            adts: union("stdlib_adts_db", Some(&reserved)),
            modules: union("stdlib_modules_db", Some(&builtins)),
            types: union("stdlib_types_db", Some(&builtins)),
            procedures: union("stdlib_procedures_db", Some(&builtins)),
            variables: union("stdlib_variables_db", Some(&builtins)),
            constants: union("stdlib_constants_db", Some(&builtins)),
            reserved,
            builtins,
        }
    }
}

fn m2_sets(lx: &LexerImpl) -> &'static [M2Sets] {
    // Only Modula2Lexer uses this, so one process-wide table is enough.
    static SETS: OnceLock<Vec<M2Sets>> = OnceLock::new();
    SETS.get_or_init(|| M2_DIALECTS.iter().map(|d| M2Sets::build(lx, d)).collect())
}

/// `get_dialect_from_dialect_tag`: the dialect named by a `(*!name*)` tag.
fn m2_dialect_from_tag(tag: &str) -> Option<usize> {
    let name = tag.strip_prefix("(*!")?.strip_suffix("*)")?;
    if tag.len() <= 5 {
        return None;
    }
    M2_DIALECTS.iter().skip(1).position(|d| *d == name).map(|i| i + 1)
}

#[derive(Clone, Debug, Default, PartialEq)]
struct M2State {
    dialect: usize,
    set_by_tag: bool,
}

fn modula2(lx: &'static LexerImpl, text: &str, tok: Tok, extra: &mut Extra, out: &mut VecDeque<Tok>) {
    let st = extra.get::<M2State>();
    let v = val(text, &tok);
    let mut ty = tok.ty;
    let tag = if !st.set_by_tag && ty == T::COMMENT_SPECIAL { m2_dialect_from_tag(v) } else { None };
    if let Some(d) = tag {
        st.dialect = d;
        st.set_by_tag = true;
    }
    let sets = &m2_sets(lx)[st.dialect];
    let dialect = M2_DIALECTS[st.dialect];
    // PIM and ISO dialects (everything but unknown, R10 and ObjM2).
    let pim_iso = !matches!(dialect, "unknown" | "m2r10" | "objm2");
    if ty == T::NAME {
        // treat_stdlib_adts_as_builtins defaults to true; algol mode is off.
        let table = [
            (&sets.reserved, T::KEYWORD_RESERVED),
            (&sets.builtins, T::NAME_BUILTIN),
            (&sets.pseudo, T::NAME_BUILTIN_PSEUDO),
            (&sets.adts, T::NAME_BUILTIN_PSEUDO),
            (&sets.modules, T::NAME_NAMESPACE),
            (&sets.types, T::NAME_CLASS),
            (&sets.procedures, T::NAME_FUNCTION),
            (&sets.variables, T::NAME_VARIABLE),
            (&sets.constants, T::NAME_CONSTANT),
        ];
        if let Some(&(_, t)) = table.iter().find(|(s, _)| s.contains(v)) {
            ty = t;
        }
    } else if ty.is_subtype_of(T::LITERAL_NUMBER) {
        if pim_iso {
            if v.contains('\'') || ["0b", "0x", "0u"].iter().any(|p| v.starts_with(p)) {
                ty = T::ERROR;
            }
        } else if dialect == "m2r10" || dialect == "objm2" {
            let bad = ty == T::LITERAL_NUMBER_OCT
                || (ty == T::LITERAL_NUMBER_HEX && v.contains('H'))
                || (ty == T::LITERAL_NUMBER_FLOAT && v.contains('E'));
            if bad {
                ty = T::ERROR;
            }
        }
    } else if ty.is_subtype_of(T::COMMENT) {
        if ty == T::COMMENT_SINGLE && pim_iso {
            ty = T::ERROR;
        }
        if ty == T::COMMENT_PREPROC {
            if v.starts_with("<*") && dialect.starts_with("m2pim") {
                ty = T::ERROR;
            } else if v.starts_with("(*$") && dialect != "unknown" && !dialect.starts_with("m2pim") {
                ty = T::COMMENT_MULTILINE;
            }
        }
    } else if sets.reject.contains(v) {
        ty = T::ERROR;
    }
    out.push_back(Tok { ty, ..tok });
}

// -- Shen: symbols after '(' and declarations ---------------------------------------

/// `ShenLexer.BUILTINS`, sorted.
const SHEN_BUILTINS: &[&str] = &[
    "*",
    "+",
    "-",
    "/",
    "<",
    "<-address",
    "<-vector",
    "<=",
    "=",
    "==",
    ">",
    ">=",
    "abort",
    "absvector",
    "absvector?",
    "address->",
    "adjoin",
    "append",
    "arity",
    "assoc",
    "bind",
    "boolean?",
    "bound?",
    "call",
    "cd",
    "close",
    "cn",
    "compile",
    "concat",
    "cons",
    "cons?",
    "cut",
    "destroy",
    "difference",
    "element?",
    "empty?",
    "enable-type-theory",
    "error-to-string",
    "eval",
    "eval-kl",
    "exception",
    "explode",
    "external",
    "fail",
    "fail-if",
    "file",
    "findall",
    "fix",
    "fst",
    "fwhen",
    "gensym",
    "get-time",
    "hash",
    "hd",
    "hdstr",
    "hdv",
    "head",
    "identical",
    "implementation",
    "in",
    "include",
    "include-all-but",
    "inferences",
    "input",
    "input+",
    "integer?",
    "intern",
    "intersection",
    "is",
    "kill",
    "language",
    "length",
    "limit",
    "lineread",
    "loaded",
    "macro",
    "macroexpand",
    "map",
    "mapcan",
    "maxinferences",
    "mode",
    "n->string",
    "nl",
    "nth",
    "null",
    "number?",
    "occurrences",
    "occurs-check",
    "open",
    "os",
    "out",
    "port",
    "porters",
    "pos",
    "pr",
    "preclude",
    "preclude-all-but",
    "print",
    "profile",
    "profile-results",
    "ps",
    "quit",
    "read",
    "read+",
    "read-byte",
    "read-file",
    "read-file-as-bytelist",
    "read-file-as-string",
    "read-from-string",
    "release",
    "remove",
    "return",
    "reverse",
    "run",
    "save",
    "set",
    "simple-error",
    "snd",
    "specialise",
    "spy",
    "step",
    "stinput",
    "stoutput",
    "str",
    "string->n",
    "string->symbol",
    "string?",
    "subst",
    "symbol?",
    "systemf",
    "tail",
    "tc",
    "tc?",
    "thaw",
    "tl",
    "tlstr",
    "tlv",
    "track",
    "tuple?",
    "undefmacro",
    "unify",
    "unify!",
    "union",
    "unprofile",
    "unspecialise",
    "untrack",
    "variable?",
    "vector",
    "vector->",
    "vector?",
    "verified",
    "version",
    "warn",
    "when",
    "write-byte",
    "write-to-file",
    "y-or-n?",
];

/// `ShenLexer.SPECIAL_FORMS`, sorted.
const SHEN_SPECIAL_FORMS: &[&str] = &[
    "$",
    "/.",
    "@p",
    "@s",
    "@v",
    "and",
    "cases",
    "cond",
    "do",
    "error",
    "freeze",
    "get",
    "if",
    "lambda",
    "let",
    "load",
    "make-string",
    "not",
    "or",
    "output",
    "prolog?",
    "protect",
    "put",
    "set",
    "time",
    "trap-error",
    "value",
];

/// `ShenLexer.MAPPINGS.get(value, Name.Function)`.
fn shen_mapping(lx: &LexerImpl, v: &str) -> T {
    if SHEN_SPECIAL_FORMS.binary_search(&v).is_ok() {
        T::KEYWORD
    } else if SHEN_BUILTINS.binary_search(&v).is_ok() {
        T::NAME_BUILTIN
    } else if has(lx, "DECLARATIONS", v) {
        T::KEYWORD
    } else {
        T::NAME_FUNCTION
    }
}

fn shen_relevant(ty: T) -> bool {
    ![T::TEXT, T::TEXT_WHITESPACE, T::COMMENT_SINGLE, T::COMMENT_MULTILINE].contains(&ty)
}

/// Python's `str.istitle()`.
fn istitle(s: &str) -> bool {
    let (mut cased, mut prev_cased) = (false, false);
    for c in s.chars() {
        if c.is_uppercase() {
            if prev_cased {
                return false;
            }
            (cased, prev_cased) = (true, true);
        } else if c.is_lowercase() {
            if !prev_cased {
                return false;
            }
            (cased, prev_cased) = (true, true);
        } else {
            prev_cased = false;
        }
    }
    cased
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum ShenDecl {
    Datatype,
    Package,
    Define,
    Other,
}

/// Where `_process_declarations` is in its nested generators.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum ShenStage {
    #[default]
    Outer,
    /// `_process_declaration`: skipping to the first relevant token.
    DeclSkip { decl: ShenDecl, seen: bool },
    /// `datatype` consumes the rest of the stream.
    Datatype { colon: bool },
    /// `define`: skipping to the token after the name; `last` was yielded last.
    DefineSkip { last: Tok },
    /// `_process_signature`, up to `}`.
    Signature,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct ShenState {
    /// `opening_paren` of `_process_symbols`.
    sym_paren: bool,
    /// `opening_paren` of `_process_declarations`.
    decl_paren: bool,
    stage: ShenStage,
}

fn shen(lx: &'static LexerImpl, text: &str, tok: Tok, extra: &mut Extra, out: &mut VecDeque<Tok>) {
    let st = extra.get::<ShenState>();
    let v = val(text, &tok);
    // _process_symbols
    let mut ty = tok.ty;
    if st.sym_paren && (ty == T::LITERAL || ty == T::NAME_VARIABLE) {
        ty = shen_mapping(lx, v);
    } else if ty == T::LITERAL && has(lx, "BUILTINS_ANYWHERE", v) {
        ty = T::NAME_BUILTIN;
    }
    st.sym_paren = v == "(" && ty == T::PUNCTUATION;
    let tok = Tok { ty, ..tok };
    let lit_to = |t: T| Tok { ty: if ty == T::LITERAL { t } else { ty }, ..tok };
    // _process_declarations
    let relevant = shen_relevant(ty);
    match st.stage {
        ShenStage::Outer => {
            out.push_back(tok);
            if relevant {
                if st.decl_paren && ty == T::KEYWORD && has(lx, "DECLARATIONS", v) {
                    let decl = match v {
                        "datatype" => ShenDecl::Datatype,
                        "package" => ShenDecl::Package,
                        "define" => ShenDecl::Define,
                        _ => ShenDecl::Other,
                    };
                    st.stage = ShenStage::DeclSkip { decl, seen: false };
                }
                st.decl_paren = v == "(" && ty == T::PUNCTUATION;
            }
        }
        ShenStage::DeclSkip { decl, .. } if !relevant => {
            out.push_back(tok);
            st.stage = ShenStage::DeclSkip { decl, seen: true };
        }
        ShenStage::DeclSkip { decl, .. } => match decl {
            ShenDecl::Datatype => {
                out.push_back(lit_to(T::KEYWORD_TYPE));
                st.stage = ShenStage::Datatype { colon: false };
            }
            ShenDecl::Package => {
                out.push_back(lit_to(T::NAME_NAMESPACE));
                st.stage = ShenStage::Outer;
            }
            ShenDecl::Define => {
                let last = lit_to(T::NAME_FUNCTION);
                out.push_back(last);
                st.stage = ShenStage::DefineSkip { last };
            }
            ShenDecl::Other => {
                out.push_back(lit_to(T::NAME_FUNCTION));
                st.stage = ShenStage::Outer;
            }
        },
        ShenStage::Datatype { colon } => {
            let t = if colon { lit_to(T::KEYWORD_TYPE) } else { tok };
            out.push_back(t);
            if shen_relevant(t.ty) {
                st.stage = ShenStage::Datatype { colon: t.ty == T::LITERAL && v == ":" };
            }
        }
        ShenStage::DefineSkip { .. } if !relevant => {
            out.push_back(tok);
            st.stage = ShenStage::DefineSkip { last: tok };
        }
        ShenStage::DefineSkip { .. } => {
            if v == "{" && ty == T::LITERAL {
                out.push_back(Tok { ty: T::PUNCTUATION, ..tok });
                st.stage = ShenStage::Signature;
            } else {
                out.push_back(tok);
                st.stage = ShenStage::Outer;
            }
        }
        ShenStage::Signature => {
            if ty == T::LITERAL && v == "}" {
                out.push_back(Tok { ty: T::PUNCTUATION, ..tok });
                st.stage = ShenStage::Outer;
            } else if ty == T::LITERAL || ty == T::NAME_FUNCTION {
                let t = if istitle(v) { T::NAME_VARIABLE } else { T::KEYWORD_TYPE };
                out.push_back(Tok { ty: t, ..tok });
            } else {
                out.push_back(tok);
            }
        }
    }
    if tok.end >= text.len() {
        // Python quirk: a generator that runs out of tokens re-yields its loop
        // variable, so the last token comes out again.
        match st.stage {
            ShenStage::DeclSkip { decl, seen: true } => {
                let copies = if decl == ShenDecl::Define { 2 } else { 1 };
                out.extend(std::iter::repeat_n(tok, copies));
            }
            ShenStage::DefineSkip { last } => out.push_back(last),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    /// `(alias, text, expected (type, value) pairs)` from Pygments 2.19.2's
    /// `get_tokens_unprocessed`, with empty tokens left out.
    #[rustfmt::skip]
    const CASES: &[(&str, &str, &[(&str, &str)])] = &[
        ("c", "size_t n; int8_t m;\n", &[("Keyword.Type", "size_t"), ("Text.Whitespace", " "), ("Name", "n"), ("Punctuation", ";"), ("Text.Whitespace", " "), ("Keyword.Type", "int8_t"), ("Text.Whitespace", " "), ("Name", "m"), ("Punctuation", ";"), ("Text.Whitespace", "\n")]),
        ("cuda", "__global__ void k() { threadIdx; float4 v; }\n", &[("Keyword.Reserved", "__global__"), ("Text.Whitespace", " "), ("Keyword.Type", "void"), ("Text.Whitespace", " "), ("Name", "k"), ("Punctuation", "("), ("Punctuation", ")"), ("Text.Whitespace", " "), ("Punctuation", "{"), ("Text.Whitespace", " "), ("Name.Builtin", "threadIdx"), ("Punctuation", ";"), ("Text.Whitespace", " "), ("Keyword.Type", "float4"), ("Text.Whitespace", " "), ("Name", "v"), ("Punctuation", ";"), ("Text.Whitespace", " "), ("Punctuation", "}"), ("Text.Whitespace", "\n")]),
        ("arduino", "void setup() { pinMode(13, OUTPUT); }\n", &[("Keyword.Reserved", "void"), ("Text.Whitespace", " "), ("Name.Builtin", "setup"), ("Punctuation", "("), ("Punctuation", ")"), ("Text.Whitespace", " "), ("Punctuation", "{"), ("Text.Whitespace", " "), ("Name.Function", "pinMode"), ("Punctuation", "("), ("Literal.Number.Integer", "13"), ("Punctuation", ","), ("Text.Whitespace", " "), ("Keyword.Reserved", "OUTPUT"), ("Punctuation", ")"), ("Punctuation", ";"), ("Text.Whitespace", " "), ("Punctuation", "}"), ("Text.Whitespace", "\n")]),
        ("freefem", "fespace Vh(Th, P1);\n", &[("Keyword.Type", "fespace"), ("Text.Whitespace", " "), ("Name.Function", "Vh"), ("Punctuation", "("), ("Name", "Th"), ("Punctuation", ","), ("Text.Whitespace", " "), ("Name.Class", "P1"), ("Punctuation", ")"), ("Punctuation", ";"), ("Text.Whitespace", "\n")]),
        ("objective-c", "NSString *s;\n", &[("Name.Builtin.Pseudo", "NSString"), ("Text.Whitespace", " "), ("Operator", "*"), ("Name", "s"), ("Punctuation", ";"), ("Text.Whitespace", "\n")]),
        ("swift", "let s: NSString = x\n", &[("Keyword.Declaration", "let"), ("Text.Whitespace", " "), ("Name.Variable", "s"), ("Punctuation", ":"), ("Text.Whitespace", " "), ("Name.Builtin.Pseudo", "NSString"), ("Text.Whitespace", " "), ("Punctuation", "="), ("Text.Whitespace", " "), ("Name", "x"), ("Text", "\n")]),
        ("aspectj", "declare parents: A;\n", &[("Keyword", "declare"), ("Text.Whitespace", " "), ("Keyword", "parents"), ("Punctuation", ":"), ("Text.Whitespace", " "), ("Name", "A"), ("Punctuation", ";"), ("Text.Whitespace", "\n")]),
        ("asymptote", "draw(unitcircle);\n", &[("Name.Function", "draw"), ("Punctuation", "("), ("Name.Variable", "unitcircle"), ("Punctuation", ")"), ("Punctuation", ";"), ("Text.Whitespace", "\n")]),
        ("common-lisp", "(car x)\n", &[("Punctuation", "("), ("Name.Builtin", "car"), ("Text.Whitespace", " "), ("Name.Variable", "x"), ("Punctuation", ")"), ("Text.Whitespace", "\n")]),
        ("emacs-lisp", "(car x)\n", &[("Punctuation", "("), ("Name.Function", "car"), ("Text.Whitespace", " "), ("Name.Variable", "x"), ("Punctuation", ")"), ("Text.Whitespace", "\n")]),
        ("scheme", "(define x (car y))\n", &[("Punctuation", "("), ("Keyword", "define"), ("Text.Whitespace", " "), ("Name.Variable", "x"), ("Text.Whitespace", " "), ("Punctuation", "("), ("Name.Builtin", "car"), ("Text.Whitespace", " "), ("Name.Variable", "y"), ("Punctuation", ")"), ("Punctuation", ")"), ("Text.Whitespace", "\n")]),
        ("cryptol", "join xs\n", &[("Name.Function", "join"), ("Text.Whitespace", " "), ("Name", "xs"), ("Text.Whitespace", "\n")]),
        ("dylan", "define method foo () end;\n", &[("Keyword", "define"), ("Text.Whitespace", " "), ("Name.Builtin", "method"), ("Text.Whitespace", " "), ("Name", "foo"), ("Text.Whitespace", " "), ("Punctuation", "("), ("Punctuation", ")"), ("Text.Whitespace", " "), ("Keyword", "end"), ("Punctuation", ";"), ("Text.Whitespace", "\n")]),
        ("elixir", "def f, do: nil\n", &[("Keyword.Declaration", "def"), ("Text.Whitespace", " "), ("Name", "f"), ("Punctuation", ","), ("Text.Whitespace", " "), ("Literal.String.Symbol", "do"), ("Punctuation", ":"), ("Text.Whitespace", " "), ("Name.Constant", "nil"), ("Text.Whitespace", "\n")]),
        ("lasso", "local(x) = string(1)\n", &[("Keyword.Declaration", "local"), ("Punctuation", "("), ("Name.Other", "x"), ("Punctuation", ")"), ("Text.Whitespace", " "), ("Operator", "="), ("Text.Whitespace", " "), ("Keyword.Type", "string"), ("Punctuation", "("), ("Literal.Number.Integer", "1"), ("Punctuation", ")"), ("Text.Whitespace", "\n")]),
        ("lua", "print(x)\n", &[("Name.Builtin", "print"), ("Punctuation", "("), ("Name.Variable", "x"), ("Punctuation", ")"), ("Text.Whitespace", "\n")]),
        ("moonscript", "a.b\n", &[("Name", "a"), ("Operator", "."), ("Name", "b"), ("Text.Whitespace", "\n")]),
        ("luau", "math.abs(x.y)\n", &[("Name.Builtin", "math"), ("Punctuation", "."), ("Name.Function", "abs"), ("Punctuation", "("), ("Name.Variable", "x"), ("Punctuation", "."), ("Name.Variable", "y"), ("Punctuation", ")"), ("Text.Whitespace", "\n")]),
        ("numpy", "array(x)\n", &[("Keyword.Pseudo", "array"), ("Punctuation", "("), ("Name", "x"), ("Punctuation", ")"), ("Text.Whitespace", "\n")]),
        ("php", "<?php strlen($x); ?>\n", &[("Comment.Preproc", "<?php"), ("Text", " "), ("Name.Builtin", "strlen"), ("Punctuation", "("), ("Name.Variable", "$x"), ("Punctuation", ");"), ("Text", " "), ("Comment.Preproc", "?>"), ("Other", "\n")]),
        ("slurm", "#SBATCH -n 1\nsrun a\n", &[("Keyword.Pseudo", "#SBATCH -n 1\n"), ("Name.Builtin", "srun"), ("Text.Whitespace", " "), ("Text", "a"), ("Text.Whitespace", "\n")]),
        ("sp", "Handle h; PrintToServer(1);\n", &[("Keyword.Type", "Handle"), ("Text", " "), ("Name", "h"), ("Punctuation", ";"), ("Text", " "), ("Name.Builtin", "PrintToServer"), ("Punctuation", "("), ("Literal.Number.Integer", "1"), ("Punctuation", ")"), ("Punctuation", ";"), ("Text", "\n")]),
        ("vim", "set nu\nfoo\n", &[("Keyword", "set"), ("Text", " "), ("Keyword", "nu"), ("Text.Whitespace", "\n"), ("Text", "foo"), ("Text.Whitespace", "\n")]),
        ("xslt", "<xsl:template match=\"/\"/>\n", &[("Keyword", "<xsl:template"), ("Text.Whitespace", " "), ("Name.Attribute", "match="), ("Literal.String", "\"/\""), ("Name.Tag", "/>"), ("Text.Whitespace", "\n")]),
        ("shen", "(define foo\n", &[("Punctuation", "("), ("Keyword", "define"), ("Text.Whitespace", " "), ("Name.Function", "foo"), ("Text.Whitespace", "\n"), ("Text.Whitespace", "\n")]),
        ("shen", "(define\n", &[("Punctuation", "("), ("Keyword", "define"), ("Text.Whitespace", "\n"), ("Text.Whitespace", "\n"), ("Text.Whitespace", "\n")]),
        ("shen", "(datatype t\n  X : number;\n)\n", &[("Punctuation", "("), ("Keyword", "datatype"), ("Text.Whitespace", " "), ("Keyword.Type", "t"), ("Text.Whitespace", "\n  "), ("Name.Variable", "X"), ("Text.Whitespace", " "), ("Literal", ":"), ("Text.Whitespace", " "), ("Keyword.Type", "number"), ("Punctuation", ";"), ("Text.Whitespace", "\n"), ("Punctuation", ")"), ("Text.Whitespace", "\n")]),
        ("shen", "(define f {number --> A} X -> X)\n", &[("Punctuation", "("), ("Keyword", "define"), ("Text.Whitespace", " "), ("Name.Function", "f"), ("Text.Whitespace", " "), ("Punctuation", "{"), ("Keyword.Type", "number"), ("Text.Whitespace", " "), ("Punctuation", "-->"), ("Text.Whitespace", " "), ("Name.Variable", "A"), ("Punctuation", "}"), ("Text.Whitespace", " "), ("Name.Variable", "X"), ("Text.Whitespace", " "), ("Punctuation", "->"), ("Text.Whitespace", " "), ("Name.Variable", "X"), ("Punctuation", ")"), ("Text.Whitespace", "\n")]),
        ("inform6", "[ f; objectloop (o in p) x; ];\n", &[("Punctuation", "["), ("Text", " "), ("Name.Function", "f"), ("Punctuation", ";"), ("Text", " "), ("Keyword", "objectloop"), ("Text", " "), ("Punctuation", "("), ("Name.Variable", "o"), ("Text", " "), ("Keyword", "in"), ("Text", " "), ("Name", "p"), ("Punctuation", ")"), ("Text", " "), ("Name", "x"), ("Punctuation", ";"), ("Text", " "), ("Punctuation", "]"), ("Punctuation", ";"), ("Text", "\n")]),
        ("inform6", "[ f; objectloop (o in p && 1) x; ];\n", &[("Punctuation", "["), ("Text", " "), ("Name.Function", "f"), ("Punctuation", ";"), ("Text", " "), ("Keyword", "objectloop"), ("Text", " "), ("Punctuation", "("), ("Name.Variable", "o"), ("Text", " "), ("Operator.Word", "in"), ("Text", " "), ("Name", "p"), ("Text", " "), ("Operator", "&&"), ("Text", " "), ("Literal.Number.Integer", "1"), ("Punctuation", ")"), ("Text", " "), ("Name", "x"), ("Punctuation", ";"), ("Text", " "), ("Punctuation", "]"), ("Punctuation", ";"), ("Text", "\n")]),
        ("tads3", "#if 0\nfoo\n#endif\nbar\n", &[("Comment.Preproc", "#if 0\n"), ("Comment", "foo"), ("Comment", "\n"), ("Comment.Preproc", "#endif\n"), ("Name.Variable", "bar"), ("Text", "\n")]),
        ("modula2", "(*!m2r10*) x := 0777B; // c\n", &[("Comment.Special", "(*!m2r10*)"), ("Text", " "), ("Name", "x"), ("Text", " "), ("Operator", ":="), ("Text", " "), ("Error", "0777B"), ("Punctuation", ";"), ("Text", " "), ("Operator", "/"), ("Operator", "/"), ("Text", " "), ("Name", "c"), ("Text", "\n")]),
        ("modula2", "MODULE m; x := 0FFH; END m.\n", &[("Keyword.Reserved", "MODULE"), ("Text", " "), ("Name", "m"), ("Punctuation", ";"), ("Text", " "), ("Name", "x"), ("Text", " "), ("Operator", ":="), ("Text", " "), ("Literal.Number.Hex", "0FFH"), ("Punctuation", ";"), ("Text", " "), ("Keyword.Reserved", "END"), ("Text", " "), ("Name", "m"), ("Punctuation", "."), ("Text", "\n")]),
    ];

    #[test]
    fn filters_match_pygments() {
        let mut failed = Vec::new();
        for (alias, text, want) in CASES {
            let lexer = crate::lexer_by_name(alias).unwrap_or_else(|| panic!("no lexer {alias}"));
            let got: Vec<(&str, &str)> =
                lexer.tokenize(text).filter(|(_, v)| !v.is_empty()).map(|(t, v)| (t.short_path(), v)).collect();
            if got != *want {
                failed.push(format!("{alias} {text:?}\n  got:  {got:?}\n  want: {want:?}"));
            }
        }
        assert!(failed.is_empty(), "{}", failed.join("\n"));
    }

    #[test]
    fn istitle_matches_python() {
        assert!(super::istitle("A"));
        assert!(super::istitle("Abc-Def"));
        assert!(!super::istitle("number"));
        assert!(!super::istitle("ABC"));
        assert!(!super::istitle("-->"));
    }

    #[test]
    fn modula2_dialect_tag() {
        assert_eq!(super::m2_dialect_from_tag("(*!m2r10*)"), Some(3));
        assert_eq!(super::m2_dialect_from_tag("(*!unknown*)"), None);
        assert_eq!(super::m2_dialect_from_tag("(* x *)"), None);
    }
}
