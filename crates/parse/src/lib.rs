//! Shell parser for explainshell WASM.
//!
//! Full bashlex port lands here (see plan item parse-crate). For now this
//! module exposes the AST surface the matcher consumes so the workspace
//! builds; the parser implementation follows next.

// TODO(owner): port bashlex tokenizer + parser -- full AST coverage

use serde::{Deserialize, Serialize};

/// Source span: byte offsets into the input command string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    /// Start offset (inclusive)
    pub start: usize,
    /// End offset (exclusive)
    pub end: usize,
}

/// Minimal shell AST node kinds the matcher walks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AstNode {
    /// Simple command: program + arguments
    Command {
        /// Source span of the whole command
        span: Span,
        /// Words in order; first is usually the program name
        words: Vec<WordNode>,
    },
    /// Shell operator (;, &, &&, ||, |, ...)
    Operator {
        /// Source span
        span: Span,
        /// Operator text
        op: String,
    },
}

/// A single word token with its source span.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WordNode {
    /// Source span
    pub span: Span,
    /// Word text with quotes removed (bashlex node.word semantics)
    pub word: String,
}
