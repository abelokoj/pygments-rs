//! A syntax highlighter that ports [Pygments](https://pygments.org) to Rust.
//!
//! The lexer rule tables, token types and styles are converted from Pygments
//! by `tools/convert.py` and embedded at compile time; no Python is needed at
//! runtime. Regexes run on Oniguruma in its Python-syntax mode, so Pygments'
//! patterns keep their meaning.
//!
//! ```
//! use pygments_rs::{lexer_by_name, TokenType};
//!
//! let rust = lexer_by_name("rust").unwrap();
//! let tokens: Vec<_> = rust.tokenize("fn main() {}\n").collect();
//! assert_eq!(tokens[0], (TokenType::KEYWORD, "fn"));
//! ```
//!
//! # Re-highlighting only edited lines
//!
//! For [resumable](Lexer::is_resumable) lexers, [`Tokens::state`] returns a
//! [`LexState`] wherever no match is half-delivered. Save it at line starts;
//! after an edit, call [`Lexer::tokenize_from`] at the first changed line
//! with the state saved there, and stop once the state at a later line start
//! equals the one you saved before the edit.

#![warn(missing_docs)]

mod analyse;
mod engine;
mod generated;
mod hooks;
mod lexer;
mod registry;
mod style;
mod token;

pub use lexer::{LexState, Lexer, Tokens};
pub use registry::{all_lexers, guess_lexer, lexer_by_name, lexer_for_filename, lexer_for_mimetype};
pub use style::{Color, Style, TokenStyle, style_by_name, style_names};
pub use token::TokenType;

/// The Pygments version the bundled data was converted from.
pub const PYGMENTS_VERSION: &str = include_str!("../PYGMENTS_VERSION").trim_ascii();

#[doc(hidden)]
pub mod coverage {
    //! Porting status, for the test suite and the README's numbers.

    /// Keys of every hand-ported hook (callbacks, filters, custom lexers,
    /// `analyse_text` functions).
    pub fn ported_keys() -> Vec<&'static str> {
        crate::hooks::ported_keys().collect()
    }
}
