//! Luau source tooling: tokenizer, syntax colorizing, block diagnostics and autocomplete.
mod check;
mod complete;
mod lexer;

pub use check::{check, Diagnostic};
pub use complete::{complete, CompKind, Completion, Completions};
pub use lexer::{colorize, tokenize, Kind, Span, Token};
