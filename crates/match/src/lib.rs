//! Matcher algorithm for explainshell WASM.
//!
//! Ports explainshell/matcher.py: walks the shell AST, resolves each command
//! to its manpage, and matches tokens to options/positionals. Full port
//! lands here (see plan item match-crate); this stub keeps the workspace
//! building with the public surface the WASM crate will call.

// TODO(owner): port Matcher -- visitcommand/visitword, positional index, nested commands, subcommands

use explainshell_core::{ExplainResult, MatchGroup};
use explainshell_data::ManpageData;
use thiserror::Error;

/// Errors from matching.
#[derive(Debug, Error)]
pub enum MatchError {
    /// No AST / empty input
    #[error("no command to match")]
    EmptyInput,

    /// Program has no manpage and no shell-level explanation applies
    #[error("unknown program: {0}")]
    UnknownProgram(String),

    /// Data lookup failed
    #[error("data error: {0}")]
    Data(#[from] explainshell_data::DataError),
}

/// Match a raw command string against the manpage data.
///
/// Full implementation walks the parsed AST; this stub resolves the first
/// word to a manpage so the API surface is exercised end to end.
pub fn explain_command(
    input: &str,
    data: &ManpageData,
) -> Result<ExplainResult, MatchError> {
    let first = input.split_whitespace().next().ok_or(MatchError::EmptyInput)?;
    let (manpage, _suggestions) = data
        .find_man_page(first)
        .map_err(|_| MatchError::UnknownProgram(first.to_string()))?;

    let mut group = MatchGroup {
        name: "command1".to_string(),
        results: Vec::new(),
        manpage: Some(manpage.clone()),
        suggestions: Vec::new(),
        error: None,
        positional_index: 0,
    };
    let _ = &mut group;

    Ok(ExplainResult {
        groups: vec![MatchGroup {
            name: "shell".to_string(),
            results: Vec::new(),
            manpage: None,
            suggestions: Vec::new(),
            error: None,
            positional_index: 0,
        }],
        expansions: Vec::new(),
    })
}
