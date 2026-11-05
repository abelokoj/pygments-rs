//! `analyse_text` heuristics used by [`crate::guess_lexer`].
//!
//! Most Pygments heuristics are a few `if <test>: return <score>` lines; the
//! converter turns those into a small IR (`data/analyse.json`) evaluated
//! here. The rest are hand-ported in [`crate::hooks`].

use std::collections::HashMap;
use std::sync::{LazyLock, OnceLock};

use onig::{Regex, SearchOptions};
use serde::Deserialize;

use crate::engine::compile;

#[derive(Deserialize)]
#[serde(untagged)]
enum Entry {
    Steps(Vec<Step>),
    Custom { custom: String },
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Step {
    If {
        #[serde(rename = "if")]
        cond: Cond,
        then: f32,
    },
    Return {
        #[serde(rename = "return")]
        value: f32,
    },
}

/// A lazily compiled Python-syntax regex.
#[derive(Default)]
struct Lazy(OnceLock<Option<Regex>>);

impl Lazy {
    fn get(&self, pat: impl FnOnce() -> String, flags: &str) -> Option<&Regex> {
        self.0.get_or_init(|| compile(&pat(), flags).ok()).as_ref()
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Cond {
    And {
        and: Vec<Cond>,
    },
    Or {
        or: Vec<Cond>,
    },
    Not {
        not: Box<Cond>,
    },
    Contains {
        contains: String,
    },
    StartsWith {
        startswith: String,
    },
    EndsWith {
        endswith: String,
    },
    LStripStartsWith {
        lstrip_startswith: String,
    },
    Shebang {
        shebang: String,
        #[serde(skip)]
        re: Lazy,
    },
    Doctype {
        doctype: String,
        #[serde(skip)]
        re: Lazy,
    },
    HtmlDoctype {
        #[allow(dead_code)]
        html_doctype: bool,
    },
    Xml {
        #[allow(dead_code)]
        xml: bool,
    },
    Search {
        search: String,
        #[serde(default)]
        flags: String,
        #[serde(skip)]
        re: Lazy,
    },
    Match {
        #[serde(rename = "match")]
        pattern: String,
        #[serde(default)]
        flags: String,
        #[serde(skip)]
        re: Lazy,
    },
}

static ENTRIES: LazyLock<HashMap<String, Entry>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../data/analyse.json")).expect("bundled analyse.json is valid")
});

/// Score `text` for the lexer with Pygments class name `class`, in `[0, 1]`.
pub(crate) fn score(class: &str, text: &str) -> f32 {
    let raw = match ENTRIES.get(class) {
        None => 0.0,
        Some(Entry::Steps(steps)) => run(steps, text),
        Some(Entry::Custom { custom }) => crate::hooks::analyse_for(custom).map_or(0.0, |f| f(text)),
    };
    // pygments.util.make_analysator clamps (and maps NaN/falsy to 0).
    if raw.is_nan() { 0.0 } else { raw.clamp(0.0, 1.0) }
}

fn run(steps: &[Step], text: &str) -> f32 {
    for step in steps {
        match step {
            Step::If { cond, then } => {
                if eval(cond, text) {
                    return *then;
                }
            }
            Step::Return { value } => return *value,
        }
    }
    0.0
}

fn eval(c: &Cond, text: &str) -> bool {
    match c {
        Cond::And { and } => and.iter().all(|c| eval(c, text)),
        Cond::Or { or } => or.iter().any(|c| eval(c, text)),
        Cond::Not { not } => !eval(not, text),
        Cond::Contains { contains } => text.contains(contains.as_str()),
        Cond::StartsWith { startswith } => text.starts_with(startswith.as_str()),
        Cond::EndsWith { endswith } => text.ends_with(endswith.as_str()),
        Cond::LStripStartsWith { lstrip_startswith } => {
            text.trim_start_matches(char::is_whitespace).starts_with(lstrip_startswith.as_str())
        }
        Cond::Shebang { shebang, re } => {
            re.get(|| format!("^{shebang}(\\.(exe|cmd|bat|bin))?$"), "i").is_some_and(|r| shebang_matches(text, r))
        }
        Cond::Doctype { doctype, re } => re.get(|| doctype.clone(), "i").is_some_and(|r| doctype_matches(text, r)),
        Cond::HtmlDoctype { .. } => html_doctype_matches(text),
        Cond::Xml { .. } => looks_like_xml(text),
        Cond::Search { search, flags, re } => re.get(|| search.clone(), flags).is_some_and(|r| r.find(text).is_some()),
        Cond::Match { pattern, flags, re } => {
            re.get(|| pattern.clone(), flags).is_some_and(|r| is_match_at_start(r, text))
        }
    }
}

fn is_match_at_start(r: &Regex, text: &str) -> bool {
    r.match_with_options(text, 0, SearchOptions::SEARCH_OPTION_NONE, None).is_some()
}

/// `pygments.util.shebang_matches`, with the already wrapped `^...$` regex.
pub(crate) fn shebang_matches(text: &str, re: &Regex) -> bool {
    let first_line = text.split('\n').next().unwrap_or("").to_lowercase();
    let Some(rest) = first_line.strip_prefix("#!") else { return false };
    let mut parts =
        rest.trim_matches(char::is_whitespace).split(['/', '\\', ' ']).filter(|x| !x.is_empty() && !x.starts_with('-'));
    parts.next_back().is_some_and(|f| re.find(f).is_some())
}

static DOCTYPE_LOOKUP: LazyLock<Regex> = LazyLock::new(|| {
    compile(
        r#"
    <!DOCTYPE\s+(
     [a-zA-Z_][a-zA-Z0-9]*
     (?: \s+      # optional in HTML5
     [a-zA-Z_][a-zA-Z0-9]*\s+
     "[^"]*")?
     )
     [^>]*>
"#,
        "smx",
    )
    .expect("valid regex")
});

/// `pygments.util.doctype_matches` (`re` must be compiled case-insensitively).
pub(crate) fn doctype_matches(text: &str, re: &Regex) -> bool {
    let mut region = onig::Region::new();
    if DOCTYPE_LOOKUP
        .search_with_options(text, 0, text.len(), SearchOptions::SEARCH_OPTION_NONE, Some(&mut region))
        .is_none()
    {
        return false;
    }
    let Some((s, e)) = region.pos(1) else { return false };
    is_match_at_start(re, text[s..e].trim_matches(char::is_whitespace))
}

/// `pygments.util.html_doctype_matches`.
pub(crate) fn html_doctype_matches(text: &str) -> bool {
    static HTML: LazyLock<Regex> = LazyLock::new(|| compile("html", "i").expect("valid regex"));
    doctype_matches(text, &HTML)
}

/// `pygments.util.looks_like_xml`.
pub(crate) fn looks_like_xml(text: &str) -> bool {
    static XML_DECL: LazyLock<Regex> = LazyLock::new(|| compile(r"\s*<\?xml[^>]*\?>", "i").expect("valid regex"));
    static TAG: LazyLock<Regex> = LazyLock::new(|| compile(r"<(.+?)(\s.*?)?>.*?</.+?>", "ism").expect("valid regex"));
    if is_match_at_start(&XML_DECL, text) || DOCTYPE_LOOKUP.find(text).is_some() {
        return true;
    }
    let head = text.char_indices().nth(1000).map_or(text, |(i, _)| &text[..i]);
    TAG.find(head).is_some()
}

/// `pygments.modeline.get_filetype_from_buffer`: a `vim: ft=...` modeline in
/// the last five or the first six lines.
pub(crate) fn modeline_filetype(text: &str) -> Option<&str> {
    static MODELINE: LazyLock<Regex> = LazyLock::new(|| {
        compile(r"(?: vi | vim | ex ) (?: [<=>]? \d* )? : .* (?: ft | filetype | syn | syntax ) = ( [^:\s]+ )", "x")
            .expect("valid regex")
    });
    let from_line = |line: &str| -> Option<(usize, usize)> {
        let mut region = onig::Region::new();
        MODELINE.search_with_options(line, 0, line.len(), SearchOptions::SEARCH_OPTION_NONE, Some(&mut region))?;
        region.pos(1)
    };
    let lines: Vec<&str> = text.lines().collect();
    let tail = lines.iter().rev().take(5).copied();
    let head = (0..=5usize).rev().filter_map(|i| lines.get(i).copied());
    tail.chain(head).find_map(|line: &str| from_line(line).map(|(s, e)| &line[s..e]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shebang() {
        let re = compile(r"^python(2\.\d)?(\.(exe|cmd|bat|bin))?$", "i").unwrap();
        assert!(shebang_matches("#!/usr/bin/env python", &re));
        assert!(shebang_matches("#!/usr/bin/python2.4", &re));
        assert!(!shebang_matches("#!/usr/bin/python-ruby", &re));
        assert!(!shebang_matches("#!/usr/bin/python/ruby", &re));
        assert!(shebang_matches("#!/usr/bin/startsomethingwith python", &re));
        assert!(shebang_matches("#!C:\\Python2.4\\Python.exe", &re));
    }

    #[test]
    fn doctype() {
        assert!(html_doctype_matches("<!DOCTYPE html>\n<html></html>"));
        assert!(!html_doctype_matches("<!DOCTYPE svg>"));
        assert!(looks_like_xml("<?xml version=\"1.0\"?><a/>"));
        assert!(looks_like_xml("<a><b>x</b></a>"));
        assert!(!looks_like_xml("plain text"));
    }

    #[test]
    fn modeline() {
        assert_eq!(modeline_filetype("x = 1\n# vim: set ft=python:\n"), Some("python"));
        assert_eq!(modeline_filetype("nothing here"), None);
    }

    #[test]
    fn bundled_ir_parses() {
        assert!(!ENTRIES.is_empty());
    }
}
