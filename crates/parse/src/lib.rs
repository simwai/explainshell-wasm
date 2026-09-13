//! Shell parser for explainshell WASM.
//!
//! Hand-written recursive-descent parser producing a bashlex-compatible
//! AST (see ast.rs). The lexer module lands next; the AST layer below is
//! shared by both.

pub mod ast;
pub mod lexer;
pub mod parser;

pub use ast::{
    AstNode, KeywordKind, RedirectSource, RedirectTarget, Span, WordNode, find_first_kind,
};
pub use lexer::{LexError, Lexer, OpKind, RawExpansion, RawWord, Token};
pub use parser::{ParseError, ParseOptions, parse, parsesingle};
