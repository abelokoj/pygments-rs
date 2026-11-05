//! The `RegexLexer` state machine, ported from `pygments.lexer`.
//!
//! A lexer is a set of named states; each state is an ordered list of rules
//! `(regex, action, transition)`. At each position the first rule whose regex
//! matches *anchored at that position* wins. This mirrors
//! `RegexLexer.get_tokens_unprocessed` and `ExtendedRegexLexer`'s variant.

use std::collections::{HashMap, VecDeque};
use std::sync::OnceLock;

use onig::{Regex, RegexOptions, Region, SearchOptions, Syntax};
use serde::Deserialize;
use serde_json::Value;

use crate::hooks::{self, CallbackFn, Cx, Extra, FilterFn, Groups};
use crate::lexer::LexerImpl;
use crate::token::TokenType;

pub(crate) type StateId = u16;

/// One emitted token: a type and a byte range into the text being lexed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Tok {
    pub ty: TokenType,
    pub start: usize,
    pub end: usize,
}

impl Tok {
    pub(crate) fn new(ty: TokenType, start: usize, end: usize) -> Tok {
        Tok { ty, start, end }
    }
}

// -- data model (the JSON written by tools/convert.py) -----------------------

#[derive(Deserialize)]
pub(crate) struct RuleData {
    re: usize,
    #[serde(default)]
    action: Option<ActionData>,
    #[serde(default)]
    push: Option<Vec<String>>,
    #[serde(default)]
    pop: Option<usize>,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum ActionData {
    Token(String),
    ByGroups {
        bygroups: Vec<Option<ActionData>>,
    },
    Using {
        using: String,
        #[serde(default)]
        stack: Option<Vec<String>>,
    },
    Callback {
        callback: String,
        #[serde(default)]
        args: Option<Value>,
    },
}

// -- compiled form ------------------------------------------------------------

pub(crate) enum Action {
    None,
    Token(TokenType),
    ByGroups(Box<[Action]>),
    Using {
        target: Target,
        stack: Option<Box<[Box<str>]>>,
    },
    Callback {
        f: CallbackFn,
        args: Value,
    },
    /// A Python callback that has not been ported yet. The match is emitted
    /// as a single `Token` so the text is preserved.
    Missing,
}

pub(crate) enum Target {
    This,
    Class(Box<str>),
}

enum PushOp {
    Pop,
    Dup,
    State(StateId),
}

enum Transition {
    None,
    Push(Box<[PushOp]>),
    Pop(usize),
}

struct Rule {
    re: u32,
    action: Action,
    transition: Transition,
}

pub(crate) struct RegexLexer {
    patterns: Box<[(Box<str>, RegexOptions)]>,
    regexes: Box<[OnceLock<Option<Regex>>]>,
    states: Box<[Box<[Rule]>]>,
    state_ids: HashMap<Box<str>, StateId>,
    root: StateId,
    /// Stack a run starts with: `[root]` unless the lexer overrides it.
    initial: Box<[StateId]>,
    pub(crate) extended: bool,
    pub(crate) filter: Option<FilterFn>,
}

fn regex_options(flags: &str) -> RegexOptions {
    let mut o = RegexOptions::REGEX_OPTION_CAPTURE_GROUP;
    for c in flags.chars() {
        o |= match c {
            'i' => RegexOptions::REGEX_OPTION_IGNORECASE,
            // Python MULTILINE: ^/$ at line breaks. Onig's Python syntax
            // defaults to SINGLELINE, so negate it.
            'm' => RegexOptions::REGEX_OPTION_NEGATE_SINGLELINE,
            // Python DOTALL is Onig's (Ruby-style) MULTILINE.
            's' => RegexOptions::REGEX_OPTION_MULTILINE,
            'x' => RegexOptions::REGEX_OPTION_EXTEND,
            _ => RegexOptions::REGEX_OPTION_NONE,
        };
    }
    o
}

/// Compile a Python-syntax pattern with Python `re` flag letters (`"imsx"`).
pub(crate) fn compile(pattern: &str, flags: &str) -> Result<Regex, onig::Error> {
    Regex::with_options(pattern, regex_options(flags), Syntax::python())
}

impl RegexLexer {
    pub(crate) fn build(
        class: &str,
        extended: bool,
        regexes: Vec<Vec<String>>,
        states: Vec<(String, Vec<RuleData>)>,
        initial_stack: &[String],
    ) -> RegexLexer {
        let patterns: Box<[(Box<str>, RegexOptions)]> = regexes
            .into_iter()
            .map(|mut p| {
                let flags = p.get(1).cloned().unwrap_or_default();
                p.truncate(1);
                (p.pop().unwrap_or_default().into_boxed_str(), regex_options(&flags))
            })
            .collect();
        let state_ids: HashMap<Box<str>, StateId> =
            states.iter().enumerate().map(|(i, (name, _))| (name.as_str().into(), i as StateId)).collect();
        let id = |name: &str| -> StateId {
            *state_ids.get(name).unwrap_or_else(|| panic!("{class}: unknown state {name:?} in bundled data"))
        };
        let compile_states: Box<[Box<[Rule]>]> = states
            .into_iter()
            .map(|(_, rules)| {
                rules
                    .into_iter()
                    .map(|r| Rule {
                        re: r.re as u32,
                        action: r.action.map_or(Action::None, |a| compile_action(class, a)),
                        transition: match (r.push, r.pop) {
                            (Some(names), _) => Transition::Push(
                                names
                                    .iter()
                                    .map(|n| match n.as_str() {
                                        "#pop" => PushOp::Pop,
                                        "#push" => PushOp::Dup,
                                        n => PushOp::State(id(n)),
                                    })
                                    .collect(),
                            ),
                            (None, Some(n)) => Transition::Pop(n),
                            (None, None) => Transition::None,
                        },
                    })
                    .collect()
            })
            .collect();
        let root = id("root");
        let initial =
            if initial_stack.is_empty() { [root].into() } else { initial_stack.iter().map(|n| id(n)).collect() };
        RegexLexer {
            regexes: patterns.iter().map(|_| OnceLock::new()).collect(),
            patterns,
            states: compile_states,
            state_ids,
            root,
            initial,
            extended,
            filter: hooks::filter_for(class),
        }
    }

    pub(crate) fn root(&self) -> StateId {
        self.root
    }

    pub(crate) fn has_state(&self, id: StateId) -> bool {
        usize::from(id) < self.states.len()
    }

    pub(crate) fn state_id(&self, name: &str) -> Option<StateId> {
        self.state_ids.get(name).copied()
    }

    /// The compiled regex, compiling it on first use. `None` if Oniguruma
    /// rejects the pattern; such a rule never matches.
    fn regex(&self, i: u32) -> Option<&Regex> {
        let i = i as usize;
        self.regexes[i]
            .get_or_init(|| {
                let (pat, opts) = &self.patterns[i];
                Regex::with_options(pat, *opts, Syntax::python()).ok()
            })
            .as_ref()
    }

    /// Compile every pattern; return `(pattern, error)` for each failure.
    pub(crate) fn regex_errors(&self) -> Vec<(String, String)> {
        self.patterns
            .iter()
            .filter_map(|(pat, opts)| {
                Regex::with_options(pat, *opts, Syntax::python())
                    .err()
                    .map(|e| (pat.to_string(), e.description().to_string()))
            })
            .collect()
    }

    pub(crate) fn stack_from_names(&self, names: &[&str]) -> Option<Vec<StateId>> {
        names.iter().map(|n| self.state_id(n)).collect()
    }
}

fn compile_action(class: &str, a: ActionData) -> Action {
    match a {
        ActionData::Token(name) => Action::Token(
            TokenType::from_name(&name)
                .unwrap_or_else(|| panic!("{class}: unknown token type {name:?} in bundled data")),
        ),
        ActionData::ByGroups { bygroups } => Action::ByGroups(
            bygroups.into_iter().map(|g| g.map_or(Action::None, |a| compile_action(class, a))).collect(),
        ),
        ActionData::Using { using, stack } => Action::Using {
            target: if using == "this" { Target::This } else { Target::Class(using.into()) },
            stack: stack.map(|s| s.into_iter().map(String::into_boxed_str).collect()),
        },
        ActionData::Callback { callback, args } => match hooks::callback_for(&callback) {
            Some(f) => Action::Callback { f, args: args.unwrap_or(Value::Null) },
            None => Action::Missing,
        },
    }
}

/// A running tokenization of one text by one regex lexer.
pub(crate) struct Run<'t> {
    pub(crate) lexer: &'static LexerImpl,
    pub(crate) text: &'t str,
    pub(crate) pos: usize,
    pub(crate) stack: Vec<StateId>,
    pub(crate) extra: Extra,
    /// Tokens produced but not yet handed out.
    pub(crate) out: VecDeque<Tok>,
    region: Region,
    scratch: Vec<Tok>,
}

impl<'t> Run<'t> {
    pub(crate) fn new(
        lexer: &'static LexerImpl,
        text: &'t str,
        pos: usize,
        stack: Vec<StateId>,
        extra: Extra,
    ) -> Run<'t> {
        Run { lexer, text, pos, stack, extra, out: VecDeque::new(), region: Region::new(), scratch: Vec::new() }
    }

    fn rl(&self) -> &'static RegexLexer {
        self.lexer.regex().expect("Run is only built for regex lexers")
    }

    /// Apply one rule (or the no-match fallback). Returns false at end of text.
    pub(crate) fn step(&mut self) -> bool {
        let rl = self.rl();
        let text = self.text;
        if self.stack.is_empty() {
            self.stack.extend_from_slice(&rl.initial);
        }
        let mut emitted = std::mem::take(&mut self.scratch);
        emitted.clear();
        let mut matched = false;
        let state = &rl.states[usize::from(*self.stack.last().expect("non-empty stack"))];
        for rule in state.iter() {
            let Some(re) = rl.regex(rule.re) else { continue };
            self.region.clear();
            let Some(len) =
                re.match_with_options(text, self.pos, SearchOptions::SEARCH_OPTION_NONE, Some(&mut self.region))
            else {
                continue;
            };
            let start = self.pos;
            let end = start + len;
            // Python would loop forever on an empty match that changes nothing;
            // treat it as a non-match so we always make progress.
            if len == 0
                && matches!(rule.transition, Transition::None)
                && !matches!(rule.action, Action::Callback { .. })
            {
                continue;
            }
            let mut new_pos = end;
            let groups = Groups::from_region(&self.region, start, end);
            match &rule.action {
                Action::None => {}
                Action::Token(t) => emitted.push(Tok::new(*t, start, end)),
                Action::Missing => emitted.push(Tok::new(TokenType::TOKEN, start, end)),
                Action::ByGroups(acts) => {
                    by_groups(self.lexer, text, acts, &groups, &mut self.stack, &mut self.extra, &mut emitted)
                }
                Action::Using { target, stack } => {
                    using(self.lexer, text, target, stack.as_deref(), start, end, &mut emitted)
                }
                Action::Callback { f, args } => {
                    let mut cx = Cx {
                        lexer: self.lexer,
                        text,
                        pos: start,
                        stack: &mut self.stack,
                        extra: &mut self.extra,
                        out: &mut emitted,
                    };
                    f(&mut cx, &groups, args);
                    // Only ExtendedRegexLexer callbacks move the position themselves.
                    if rl.extended {
                        new_pos = cx.pos;
                    }
                }
            }
            self.pos = new_pos;
            match &rule.transition {
                Transition::None => {}
                Transition::Push(ops) => {
                    for op in ops.iter() {
                        match op {
                            PushOp::Pop => {
                                if self.stack.len() > 1 {
                                    self.stack.pop();
                                }
                            }
                            PushOp::Dup => {
                                let top = *self.stack.last().expect("non-empty stack");
                                self.stack.push(top);
                            }
                            PushOp::State(s) => self.stack.push(*s),
                        }
                    }
                }
                Transition::Pop(n) => {
                    let keep = if *n >= self.stack.len() { 1 } else { self.stack.len() - n };
                    self.stack.truncate(keep);
                }
            }
            if self.stack.is_empty() {
                self.stack.push(rl.root);
            }
            matched = true;
            break;
        }
        if !matched {
            let Some(c) = text[self.pos..].chars().next() else {
                self.scratch = emitted;
                return false;
            };
            let start = self.pos;
            self.pos += c.len_utf8();
            if c == '\n' {
                self.stack.clear();
                self.stack.push(rl.root);
                let ty = if rl.extended { TokenType::TEXT } else { TokenType::TEXT_WHITESPACE };
                emitted.push(Tok::new(ty, start, self.pos));
            } else {
                emitted.push(Tok::new(TokenType::ERROR, start, self.pos));
            }
        }
        match rl.filter {
            Some(filter) => {
                for tok in emitted.drain(..) {
                    filter(self.lexer, text, tok, &mut self.extra, &mut self.out);
                }
            }
            None => self.out.extend(emitted.drain(..)),
        }
        self.scratch = emitted;
        true
    }

    /// Run to the end, appending every token to `out`.
    pub(crate) fn run_into(mut self, out: &mut Vec<Tok>) {
        while self.step() {
            out.extend(self.out.drain(..));
        }
        out.extend(self.out.drain(..));
    }
}

/// `bygroups(...)`: one action per capture group.
pub(crate) fn by_groups(
    lexer: &'static LexerImpl,
    text: &str,
    acts: &[Action],
    groups: &Groups<'_>,
    stack: &mut Vec<StateId>,
    extra: &mut Extra,
    out: &mut Vec<Tok>,
) {
    for (i, act) in acts.iter().enumerate() {
        let Some((s, e)) = groups.get(i + 1) else { continue };
        match act {
            Action::None => {}
            Action::Token(t) => {
                if e > s {
                    out.push(Tok::new(*t, s, e));
                }
            }
            Action::Missing => out.push(Tok::new(TokenType::TOKEN, s, e)),
            Action::ByGroups(inner) => {
                // Pygments passes a group-less pseudo match here.
                by_groups(lexer, text, inner, &Groups::pseudo(s, e), stack, extra, out)
            }
            Action::Using { target, stack: st } => using(lexer, text, target, st.as_deref(), s, e, out),
            Action::Callback { f, args } => {
                let mut cx = Cx { lexer, text, pos: s, stack, extra, out };
                f(&mut cx, &Groups::pseudo(s, e), args);
            }
        }
    }
}

/// `using(other, state=...)`: lex `text[start..end]` as a fresh text with
/// another lexer (or this one), shifting offsets back.
pub(crate) fn using(
    lexer: &'static LexerImpl,
    text: &str,
    target: &Target,
    stack: Option<&[Box<str>]>,
    start: usize,
    end: usize,
    out: &mut Vec<Tok>,
) {
    let target = match target {
        Target::This => lexer,
        Target::Class(c) => match crate::registry::by_class(c) {
            Some(l) => l,
            None => {
                out.push(Tok::new(TokenType::TOKEN, start, end));
                return;
            }
        },
    };
    let names: Option<Vec<&str>> = stack.map(|s| s.iter().map(|n| &**n).collect());
    let first = out.len();
    target.tokens_unprocessed(&text[start..end], names.as_deref(), out);
    for t in &mut out[first..] {
        t.start += start;
        t.end += start;
    }
}
