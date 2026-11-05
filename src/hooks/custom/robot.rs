//! `RobotFrameworkLexer` from `pygments.lexers.robotframework`: row splitting,
//! per-table tokenizers and `${variable}` splitting.

use std::sync::LazyLock;

use onig::Regex;

use super::{match_at, py_lines, py_strip};
use crate::engine::{Tok, compile};
use crate::lexer::LexerImpl;
use crate::token::TokenType;

const HEADING: TokenType = TokenType::GENERIC_HEADING;
const SETTING: TokenType = TokenType::KEYWORD_NAMESPACE;
const IMPORT: TokenType = TokenType::NAME_NAMESPACE;
const TC_KW_NAME: TokenType = TokenType::GENERIC_SUBHEADING;
const KEYWORD: TokenType = TokenType::NAME_FUNCTION;
const ARGUMENT: TokenType = TokenType::LITERAL_STRING;
const VARIABLE: TokenType = TokenType::NAME_VARIABLE;
const COMMENT: TokenType = TokenType::COMMENT;
const SEPARATOR: TokenType = TokenType::PUNCTUATION;
const SYNTAX: TokenType = TokenType::PUNCTUATION;
const GHERKIN: TokenType = TokenType::GENERIC_EMPH;
const ERROR: TokenType = TokenType::ERROR;

static PIPE_SPLITTER: LazyLock<Regex> =
    LazyLock::new(|| compile(r"((?:^| +)\|(?: +|$))", "").expect("static Robot pattern"));
static GHERKIN_PREFIX: LazyLock<Regex> =
    LazyLock::new(|| compile(r"^(Given|When|Then|And|But) ", "i").expect("static Robot pattern"));

type Out<'t> = Vec<(&'t str, TokenType)>;

fn normalize(s: &str, remove: &str) -> String {
    s.to_lowercase().chars().filter(|&c| c != ' ' && !remove.contains(c)).collect()
}

pub(super) fn robot(_lexer: &'static LexerImpl, text: &str, out: &mut Vec<Tok>) {
    let mut rows = RowTokenizer::new();
    let mut index = 0;
    let mut values: Out<'_> = Vec::new();
    for (s, ce, e) in py_lines(text) {
        values.clear();
        // Python yields '\n' for the line break; use the real one (empty at EOF).
        rows.tokenize(&text[s..ce], &text[ce..e], &mut values);
        for &(value, token) in &values {
            var_tokenize(value, token, &mut |v: &str, t| {
                if !v.is_empty() {
                    out.push(Tok::new(t, index, index + v.len()));
                    index += v.len();
                }
            });
        }
    }
}

// -- VariableTokenizer / VariableSplitter ---------------------------------------

fn var_tokenize(s: &str, token: TokenType, emit: &mut dyn FnMut(&str, TokenType)) {
    let var = match split_variable(s, b"$@%&") {
        Some(v) if token != COMMENT && token != ERROR => v,
        _ => {
            emit(s, token);
            return;
        }
    };
    emit(&s[..var.start], token);
    emit(&s[var.start..var.start + 2], SYNTAX);
    var_tokenize(&s[var.base.0..var.base.1], VARIABLE, emit);
    emit(&s[var.base.1..var.base.1 + 1], SYNTAX);
    if let Some((is, ie)) = var.index {
        emit(&s[is - 1..is], SYNTAX);
        var_tokenize(&s[is..ie], VARIABLE, emit);
        emit(&s[ie..ie + 1], SYNTAX);
    }
    var_tokenize(&s[var.end..], token, emit);
}

/// Result of Robot Framework's `VariableSplitter`, as byte offsets (all the
/// syntax characters are ASCII, so bytes behave like Python's chars).
struct VarSplit {
    start: usize,
    end: usize,
    base: (usize, usize),
    index: Option<(usize, usize)>,
}

fn is_escaped(s: &[u8], mut index: usize) -> bool {
    let mut escaped = false;
    while index > 0 && s[index - 1] == b'\\' {
        index -= 1;
        escaped = !escaped;
    }
    escaped
}

fn find_variable(s: &[u8], ids: &[u8]) -> Option<(usize, usize)> {
    let mut limit = s.len();
    loop {
        let max_end = s[..limit].iter().rposition(|&b| b == b'}')?;
        if is_escaped(s, max_end) {
            limit = max_end;
            continue;
        }
        // _find_start_index(string, 1, max_end)
        let mut from = 1;
        loop {
            if from >= max_end {
                return None;
            }
            let brace = from + s[from..max_end].iter().position(|&b| b == b'{')?;
            let index = brace - 1;
            if ids.contains(&s[index]) && !is_escaped(s, index) {
                return Some((index, max_end));
            }
            from = index + 2;
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum VState {
    Variable,
    InternalStart,
    WaitingIndex,
    Index,
}

fn split_variable(string: &str, ids: &[u8]) -> Option<VarSplit> {
    let s = string.as_bytes();
    let (start, max_index) = find_variable(s, ids)?;
    let list_or_dict = matches!(s[start], b'@' | b'&');
    let mut open_curly = 1;
    let mut state = VState::Variable;
    let mut var_len = 2;
    let mut index_len = 0;
    let mut index_last = 0u8;
    for (index, &c) in s.iter().enumerate().skip(start + 2) {
        match state {
            VState::InternalStart if c == b'{' => {
                state = VState::Variable;
                var_len += 1;
                open_curly += 1;
            }
            // `_variable_state`, also reached from `_internal_variable_start_state`.
            VState::Variable | VState::InternalStart => {
                state = VState::Variable;
                var_len += 1;
                if c == b'}' && !is_escaped(s, index) {
                    open_curly -= 1;
                    if open_curly == 0 {
                        if !list_or_dict {
                            break;
                        }
                        state = VState::WaitingIndex;
                    }
                } else if ids.contains(&c) {
                    state = VState::InternalStart;
                }
            }
            VState::WaitingIndex => {
                if c != b'[' {
                    break;
                }
                index_len += 1;
                index_last = c;
                state = VState::Index;
            }
            VState::Index => {
                index_len += 1;
                index_last = c;
                if c == b']' {
                    break;
                }
            }
        }
        if index == max_index && !matches!(state, VState::WaitingIndex | VState::Index) {
            break;
        }
    }
    let mut end = start + var_len;
    let base = (start + 2, end - 1);
    let mut idx = None;
    if index_len > 0 && index_last == b']' {
        idx = Some((end + 1, end + index_len - 1));
        end += index_len;
    }
    Some(VarSplit { start, end, base, index: idx })
}

fn is_assign(value: &str) -> bool {
    let v = match value.strip_suffix('=') {
        Some(v) => py_strip(v),
        None => value,
    };
    split_variable(v, b"$@&").is_some_and(|var| var.start == 0 && var.end == v.len())
}

fn gherkin<'t>(value: &'t str, token: TokenType, out: &mut Out<'t>) {
    match match_at(&GHERKIN_PREFIX, value, 0) {
        Some(end) => {
            out.push((&value[..end], GHERKIN));
            out.push((&value[end..], token));
        }
        None => out.push((value, token)),
    }
}

// -- tokenizers ------------------------------------------------------------------

/// Template state of the test case table (`_test_template`, `_default_template`).
#[derive(Default)]
struct Templates {
    test: Option<bool>,
    default: Option<bool>,
}

fn is_template_set(t: &str) -> bool {
    !matches!(normalize(t, "").as_str(), "" | "\\" | "none" | "${empty}")
}

#[derive(Clone, Copy, PartialEq)]
enum Setter {
    Default,
    Test,
}

#[derive(Clone, Copy, PartialEq)]
enum SettingKind {
    Setting,
    TestCase,
    Keyword,
}

impl SettingKind {
    fn keyword_settings(self) -> &'static [&'static str] {
        match self {
            SettingKind::Setting => &[
                "suitesetup",
                "suiteprecondition",
                "suiteteardown",
                "suitepostcondition",
                "testsetup",
                "tasksetup",
                "testprecondition",
                "testteardown",
                "taskteardown",
                "testpostcondition",
                "testtemplate",
                "tasktemplate",
            ],
            SettingKind::TestCase => &["setup", "precondition", "teardown", "postcondition", "template"],
            SettingKind::Keyword => &["teardown"],
        }
    }

    fn import_settings(self) -> &'static [&'static str] {
        match self {
            SettingKind::Setting => &["library", "resource", "variables"],
            _ => &[],
        }
    }

    fn other_settings(self) -> &'static [&'static str] {
        match self {
            SettingKind::Setting => {
                &["documentation", "metadata", "forcetags", "defaulttags", "testtimeout", "tasktimeout"]
            }
            SettingKind::TestCase => &["documentation", "tags", "timeout"],
            SettingKind::Keyword => &["documentation", "arguments", "return", "timeout", "tags"],
        }
    }
}

enum Kind {
    Comment,
    Setting { kind: SettingKind, setter: Option<Setter>, custom: Option<Box<Tokenizer>> },
    Import,
    Variable,
    KeywordCall { found: bool, assigns: usize },
    Templated,
    ForLoop { in_arguments: bool },
}

struct Tokenizer {
    index: usize,
    kind: Kind,
}

impl Tokenizer {
    fn new(kind: Kind) -> Tokenizer {
        Tokenizer { index: 0, kind }
    }

    fn setting(kind: SettingKind, setter: Option<Setter>) -> Tokenizer {
        Tokenizer::new(Kind::Setting { kind, setter, custom: None })
    }

    fn keyword_call(support_assign: bool) -> Tokenizer {
        Tokenizer::new(Kind::KeywordCall { found: !support_assign, assigns: 0 })
    }

    fn tokenize<'t>(&mut self, value: &'t str, tpl: &mut Templates, out: &mut Out<'t>) {
        let index = self.index;
        self.index += 1;
        let pick = |a: TokenType, b: TokenType, i: usize| if i == 0 { a } else { b };
        match &mut self.kind {
            Kind::Comment => out.push((value, COMMENT)),
            Kind::Import => out.push((value, pick(IMPORT, ARGUMENT, index))),
            Kind::Templated => out.push((value, ARGUMENT)),
            Kind::Variable => {
                let t = if index == 0 && !is_assign(value) { ERROR } else { pick(SYNTAX, ARGUMENT, index) };
                out.push((value, t));
            }
            Kind::KeywordCall { found, assigns } => {
                if !*found && is_assign(value) {
                    *assigns += 1;
                    out.push((value, SYNTAX));
                } else if *found {
                    out.push((value, pick(KEYWORD, ARGUMENT, index.saturating_sub(*assigns))));
                } else {
                    *found = true;
                    gherkin(value, KEYWORD, out);
                }
            }
            Kind::ForLoop { in_arguments } => {
                let t = if *in_arguments { ARGUMENT } else { SYNTAX };
                if matches!(value.to_uppercase().as_str(), "IN" | "IN RANGE") {
                    *in_arguments = true;
                }
                out.push((value, t));
            }
            Kind::Setting { kind, setter, custom } => {
                if *kind != SettingKind::Setting && index == 0 {
                    // TestCaseSetting: `[Name]`.
                    let inner = value.get(1..value.len().saturating_sub(1)).unwrap_or("");
                    let t = setting_head(*kind, custom, inner);
                    out.push(("[", SYNTAX));
                    out.push((inner, t));
                    out.push(("]", SYNTAX));
                    return;
                }
                if index == 1 {
                    match setter {
                        Some(Setter::Default) => tpl.default = Some(is_template_set(value)),
                        Some(Setter::Test) => tpl.test = Some(is_template_set(value)),
                        None => {}
                    }
                }
                if index == 0 {
                    out.push((value, setting_head(*kind, custom, value)));
                } else if let Some(c) = custom {
                    c.tokenize(value, tpl, out);
                } else {
                    out.push((value, ARGUMENT));
                }
            }
        }
    }
}

/// `Setting._tokenize` at index 0: pick the custom tokenizer, return the type.
fn setting_head(kind: SettingKind, custom: &mut Option<Box<Tokenizer>>, value: &str) -> TokenType {
    let n = normalize(value, "");
    if kind.keyword_settings().contains(&n.as_str()) {
        *custom = Some(Box::new(Tokenizer::keyword_call(false)));
    } else if kind.import_settings().contains(&n.as_str()) {
        *custom = Some(Box::new(Tokenizer::new(Kind::Import)));
    } else if !kind.other_settings().contains(&n.as_str()) {
        return ERROR;
    }
    SETTING
}

// -- tables ----------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum TableKind {
    Unknown,
    Variable,
    Setting,
    TestCase,
    Keyword,
}

struct Table {
    kind: TableKind,
    tokenizer: Tokenizer,
    prev_tokenizer: Option<Tokenizer>,
    /// `all(_is_empty(v) for v in _prev_values_on_row)`
    prev_all_empty: bool,
}

impl Table {
    fn new(kind: TableKind, tpl: &Templates) -> Table {
        Table { kind, tokenizer: Table::fresh_tokenizer(kind, tpl), prev_tokenizer: None, prev_all_empty: true }
    }

    fn fresh_tokenizer(kind: TableKind, tpl: &Templates) -> Tokenizer {
        match kind {
            TableKind::Unknown => Tokenizer::new(Kind::Comment),
            TableKind::Variable => Tokenizer::new(Kind::Variable),
            TableKind::Setting => Tokenizer::setting(SettingKind::Setting, None),
            TableKind::TestCase => {
                if tpl.test == Some(true) || (tpl.default == Some(true) && tpl.test != Some(false)) {
                    Tokenizer::new(Kind::Templated)
                } else {
                    Tokenizer::keyword_call(true)
                }
            }
            TableKind::Keyword => Tokenizer::keyword_call(true),
        }
    }

    fn is_empty(value: &str) -> bool {
        value.is_empty() || value == "\\"
    }

    fn tokenize<'t>(&mut self, value: &'t str, index: usize, tpl: &mut Templates, out: &mut Out<'t>) {
        let continues = match self.kind {
            TableKind::Unknown => false,
            TableKind::TestCase | TableKind::Keyword => index > 0 && value == "..." && self.prev_all_empty,
            _ => value == "..." && self.prev_all_empty,
        };
        if continues {
            // Python would crash on a missing previous tokenizer; keep ours.
            if let Some(p) = self.prev_tokenizer.take() {
                self.tokenizer = p;
            }
            out.push((value, SYNTAX));
        } else {
            self.tokenize_value(value, index, tpl, out);
        }
        self.prev_all_empty &= Table::is_empty(value);
    }

    fn tokenize_value<'t>(&mut self, value: &'t str, index: usize, tpl: &mut Templates, out: &mut Out<'t>) {
        match self.kind {
            TableKind::Setting => {
                if index == 0 && normalize(value, "") == "testtemplate" {
                    self.tokenizer = Tokenizer::setting(SettingKind::Setting, Some(Setter::Default));
                }
            }
            TableKind::TestCase | TableKind::Keyword => {
                let testcase = self.kind == TableKind::TestCase;
                if index == 0 {
                    if !value.is_empty() && testcase {
                        tpl.test = None;
                    }
                    gherkin(value, TC_KW_NAME, out);
                    return;
                }
                if index == 1 && value.starts_with('[') && value.ends_with(']') {
                    if testcase && normalize(value, "") == "[template]" {
                        tpl.test = Some(false);
                        self.tokenizer = Tokenizer::setting(SettingKind::TestCase, Some(Setter::Test));
                    } else {
                        let kind = if testcase { SettingKind::TestCase } else { SettingKind::Keyword };
                        self.tokenizer = Tokenizer::setting(kind, None);
                    }
                }
                if index == 1 && value.starts_with(':') && normalize(value, ":") == "for" {
                    self.tokenizer = Tokenizer::new(Kind::ForLoop { in_arguments: false });
                }
                if index == 1 && Table::is_empty(value) {
                    out.push((value, SYNTAX));
                    return;
                }
            }
            _ => {}
        }
        self.tokenizer.tokenize(value, tpl, out);
    }

    fn end_row(&mut self, tpl: &Templates) {
        let fresh = Table::fresh_tokenizer(self.kind, tpl);
        self.prev_tokenizer = Some(std::mem::replace(&mut self.tokenizer, fresh));
        self.prev_all_empty = true;
    }
}

struct RowTokenizer {
    tables: [Table; 5],
    current: usize,
    tpl: Templates,
}

const UNKNOWN: usize = 0;

impl RowTokenizer {
    fn new() -> RowTokenizer {
        let tpl = Templates::default();
        let tables = [
            Table::new(TableKind::Unknown, &tpl),
            Table::new(TableKind::Setting, &tpl),
            Table::new(TableKind::Variable, &tpl),
            Table::new(TableKind::TestCase, &tpl),
            Table::new(TableKind::Keyword, &tpl),
        ];
        RowTokenizer { tables, current: UNKNOWN, tpl }
    }

    fn start_table(&mut self, header: &str) {
        self.current = match normalize(header, "*").as_str() {
            "settings" | "setting" | "metadata" => 1,
            "variables" | "variable" => 2,
            "testcases" | "testcase" | "tasks" | "task" => 3,
            "keywords" | "keyword" | "userkeywords" | "userkeyword" => 4,
            _ => {
                self.tables[UNKNOWN] = Table::new(TableKind::Unknown, &self.tpl);
                UNKNOWN
            }
        };
    }

    fn tokenize<'t>(&mut self, row: &'t str, newline: &'t str, out: &mut Out<'t>) {
        let mut cells = Vec::new();
        split_row(row, &mut cells);
        cells.push(newline);
        let mut commented = false;
        let mut heading = false;
        for (i, value) in cells.into_iter().enumerate() {
            // First value, and every second after that, is a separator.
            let separator = i % 2 == 0;
            let index = i.saturating_sub(1) / 2;
            if value.starts_with('#') {
                commented = true;
            } else if !separator && index == 0 && value.starts_with('*') {
                self.start_table(value);
                heading = true;
            }
            if commented {
                out.push((value, COMMENT));
            } else if separator {
                out.push((value, SEPARATOR));
            } else if heading {
                out.push((value, HEADING));
            } else {
                self.tables[self.current].tokenize(value, index, &mut self.tpl, out);
            }
        }
        self.tables[self.current].end_row(&self.tpl);
    }
}

/// `RowSplitter.split` without the trailing `'\n'`.
fn split_row<'t>(row: &'t str, out: &mut Vec<&'t str>) {
    if row.starts_with("| ") {
        let mut rest = row;
        let mut first = true;
        // `^` anchors at the start of each remaining piece, as in Python.
        while let Some((a, b)) = PIPE_SPLITTER.find(rest) {
            if !first {
                out.push(&rest[..a]);
            }
            first = false;
            out.push(&rest[a..b]);
            rest = &rest[b..];
        }
        out.push(rest);
    } else {
        // '' pseudo separator, then re.split('( {2,})', row).
        out.push(&row[..0]);
        let b = row.as_bytes();
        let mut cell = 0;
        let mut i = 0;
        while i < b.len() {
            if b[i] == b' ' {
                let mut j = i;
                while j < b.len() && b[j] == b' ' {
                    j += 1;
                }
                if j - i >= 2 {
                    out.push(&row[cell..i]);
                    out.push(&row[i..j]);
                    cell = j;
                }
                i = j;
            } else {
                i += 1;
            }
        }
        out.push(&row[cell..]);
    }
}

#[cfg(test)]
mod tests {
    use super::super::{exp, lex};

    #[test]
    fn robot_tables() {
        assert_eq!(
            lex(
                "robotframework",
                "*** Settings ***\nLibrary    OperatingSystem\nTest Template    Foo\n\n*** Variables ***\n${X}    1\n@{L}    a    b\n\n*** Test Cases ***\nT1\n    [Documentation]    doc\n    Given foo    ${L}[0]    # c\n    ...    more\n"
            ),
            exp(&[
                ("Generic.Heading", "*** Settings ***"),
                ("Punctuation", "\n"),
                ("Keyword.Namespace", "Library"),
                ("Punctuation", "    "),
                ("Name.Namespace", "OperatingSystem"),
                ("Punctuation", "\n"),
                ("Keyword.Namespace", "Test Template"),
                ("Punctuation", "    "),
                ("Name.Function", "Foo"),
                ("Punctuation", "\n"),
                ("Punctuation", "\n"),
                ("Generic.Heading", "*** Variables ***"),
                ("Punctuation", "\n"),
                ("Punctuation", "${"),
                ("Name.Variable", "X"),
                ("Punctuation", "}"),
                ("Punctuation", "    "),
                ("Literal.String", "1"),
                ("Punctuation", "\n"),
                ("Punctuation", "@{"),
                ("Name.Variable", "L"),
                ("Punctuation", "}"),
                ("Punctuation", "    "),
                ("Literal.String", "a"),
                ("Punctuation", "    "),
                ("Literal.String", "b"),
                ("Punctuation", "\n"),
                ("Punctuation", "\n"),
                ("Generic.Heading", "*** Test Cases ***"),
                ("Punctuation", "\n"),
                ("Generic.Subheading", "T1"),
                ("Punctuation", "\n"),
                ("Punctuation", "    "),
                ("Punctuation", "["),
                ("Keyword.Namespace", "Documentation"),
                ("Punctuation", "]"),
                ("Punctuation", "    "),
                ("Literal.String", "doc"),
                ("Punctuation", "\n"),
                ("Punctuation", "    "),
                ("Literal.String", "Given foo"),
                ("Punctuation", "    "),
                ("Punctuation", "${"),
                ("Name.Variable", "L"),
                ("Punctuation", "}"),
                ("Literal.String", "[0]"),
                ("Punctuation", "    "),
                ("Comment", "# c"),
                ("Comment", "\n"),
                ("Punctuation", "    "),
                ("Punctuation", "..."),
                ("Punctuation", "    "),
                ("Literal.String", "more"),
                ("Punctuation", "\n"),
            ])
        );
    }

    #[test]
    fn robot_keywords_pipes() {
        assert_eq!(
            lex(
                "robotframework",
                "*** Keywords ***\n| My Kw | ${a}= | Set Var | x${y${z}} |\n|  | :FOR | ${i} | IN RANGE | 3\n| | [Teardown] | Log | \\${esc}\n"
            ),
            exp(&[
                ("Generic.Heading", "*** Keywords ***"),
                ("Punctuation", "\n"),
                ("Punctuation", "| "),
                ("Generic.Subheading", "My Kw"),
                ("Punctuation", " | "),
                ("Punctuation", "${"),
                ("Name.Variable", "a"),
                ("Punctuation", "}"),
                ("Punctuation", "="),
                ("Punctuation", " | "),
                ("Name.Function", "Set Var"),
                ("Punctuation", " | "),
                ("Literal.String", "x"),
                ("Punctuation", "${"),
                ("Name.Variable", "y"),
                ("Punctuation", "${"),
                ("Name.Variable", "z"),
                ("Punctuation", "}"),
                ("Punctuation", "}"),
                ("Punctuation", " |"),
                ("Punctuation", "\n"),
                ("Punctuation", "|  "),
                ("Punctuation", "| "),
                ("Punctuation", ":FOR"),
                ("Punctuation", " | "),
                ("Punctuation", "${"),
                ("Name.Variable", "i"),
                ("Punctuation", "}"),
                ("Punctuation", " | "),
                ("Punctuation", "IN RANGE"),
                ("Punctuation", " | "),
                ("Literal.String", "3"),
                ("Punctuation", "\n"),
                ("Punctuation", "| "),
                ("Punctuation", "| "),
                ("Punctuation", "["),
                ("Keyword.Namespace", "Teardown"),
                ("Punctuation", "]"),
                ("Punctuation", " | "),
                ("Name.Function", "Log"),
                ("Punctuation", " | "),
                ("Literal.String", "\\${esc}"),
                ("Punctuation", "\n"),
            ])
        );
    }
}
