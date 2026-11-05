//! Pygments' token type hierarchy (`Token.Keyword.Constant` and so on).

use std::collections::HashMap;
use std::fmt;
use std::sync::OnceLock;

use crate::generated::tokens::TOKENS;

/// A token type such as `Token.Keyword.Constant`.
///
/// Token types form a tree rooted at [`TokenType::TOKEN`]; a style that
/// colors `Keyword` also colors `Keyword.Constant` unless the child overrides
/// it. Values are small copyable handles into a table generated from Pygments.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TokenType(pub(crate) u16);

impl TokenType {
    /// Full dotted name, e.g. `"Token.Keyword.Constant"`.
    pub fn name(self) -> &'static str {
        TOKENS[usize::from(self.0)].0
    }

    /// Name without the leading `Token.`, as Pygments' test files print it
    /// (`"Keyword.Constant"`; the root is `""`).
    pub fn short_path(self) -> &'static str {
        self.name().strip_prefix("Token.").unwrap_or("")
    }

    /// Pygments' short CSS class (`"kc"` for `Keyword.Constant`), empty if
    /// the type is not one of Pygments' standard types.
    pub fn css_class(self) -> &'static str {
        TOKENS[usize::from(self.0)].2
    }

    /// The parent type, or `None` for the root [`TokenType::TOKEN`].
    pub fn parent(self) -> Option<TokenType> {
        (self.0 != 0).then(|| TokenType(TOKENS[usize::from(self.0)].1))
    }

    /// This type followed by each ancestor up to the root.
    pub fn ancestors(self) -> impl Iterator<Item = TokenType> {
        std::iter::successors(Some(self), |t| t.parent())
    }

    /// True if `self` is `other` or a descendant of it (Pygments' `in`).
    pub fn is_subtype_of(self, other: TokenType) -> bool {
        self.ancestors().any(|t| t == other)
    }

    /// Look a type up by its dotted name, with or without the `Token.` prefix.
    pub fn from_name(name: &str) -> Option<TokenType> {
        static BY_NAME: OnceLock<HashMap<&'static str, TokenType>> = OnceLock::new();
        let map = BY_NAME.get_or_init(|| TOKENS.iter().enumerate().map(|(i, t)| (t.0, TokenType(i as u16))).collect());
        let full;
        let key = if name == "Token" || name.starts_with("Token.") {
            name
        } else if name.is_empty() {
            "Token"
        } else {
            full = format!("Token.{name}");
            &full
        };
        map.get(key).copied()
    }

    /// Every token type known to the library.
    pub fn all() -> impl Iterator<Item = TokenType> {
        (0..TOKENS.len() as u16).map(TokenType)
    }
}

impl fmt::Debug for TokenType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl fmt::Display for TokenType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hierarchy() {
        let kc = TokenType::KEYWORD_CONSTANT;
        assert_eq!(kc.name(), "Token.Keyword.Constant");
        assert_eq!(kc.short_path(), "Keyword.Constant");
        assert_eq!(kc.css_class(), "kc");
        assert_eq!(kc.parent(), Some(TokenType::KEYWORD));
        assert!(kc.is_subtype_of(TokenType::KEYWORD));
        assert!(!TokenType::KEYWORD.is_subtype_of(kc));
        assert_eq!(TokenType::TOKEN.parent(), None);
        assert_eq!(TokenType::from_name("Keyword.Constant"), Some(kc));
        assert_eq!(TokenType::from_name("Token.Keyword.Constant"), Some(kc));
        assert_eq!(TokenType::from_name("Token"), Some(TokenType::TOKEN));
        assert_eq!(TokenType::from_name("Nope.Nope"), None);
    }
}
