//! Core domain types for explainshell WASM
//!
//! These types mirror the Python explainshell models and are used across
//! the extraction pipeline, matcher, and WASM exports.

use serde::{Deserialize, Serialize};

/// An extracted command-line option from a man page.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct CliOption {
    /// Human-readable help text for this option
    pub text: String,
    /// Short options (e.g., "-a", "-b")
    #[serde(default)]
    pub short: Vec<String>,
    /// Long options (e.g., "--all", "--verbose")
    #[serde(default)]
    pub long: Vec<String>,
    /// Whether the option expects an argument
    #[serde(default)]
    pub has_argument: HasArgument,
    /// Positional argument specification
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub positional: Option<Positional>,
    /// Literal prefix sigil for positional (e.g., "@" for dig @server)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    /// Whether this option can start a nested command
    #[serde(default)]
    pub nested_cmd: NestedCmd,
    /// Arbitrary metadata from extraction
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
}

impl CliOption {
    /// Returns all option names (short + long)
    pub fn opts(&self) -> Vec<String> {
        let mut opts = self.short.clone();
        opts.extend(self.long.clone());
        opts
    }

    /// Check if this option matches a given flag
    pub fn matches_flag(&self, flag: &str) -> bool {
        self.opts().iter().any(|o| o == flag)
    }
}

/// Whether an option expects an argument
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum HasArgument {
    /// Simple boolean: true = expects argument, false = no argument
    Bool(bool),
    /// List of possible argument values
    List(Vec<String>),
}

impl Default for HasArgument {
    fn default() -> Self {
        HasArgument::Bool(false)
    }
}

/// Positional argument specification
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Positional {
    /// Named positional (e.g., "file", "pattern")
    Name(String),
    /// Boolean positional flag
    Bool(bool),
}

/// Nested command specification
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum NestedCmd {
    /// Simple boolean
    Bool(bool),
    /// Single end-word that terminates the nested command
    /// (Python stores manpage-level nested_cmd as bool | str)
    Str(String),
    /// List of end-words that terminate the nested command
    List(Vec<String>),
}

impl Default for NestedCmd {
    fn default() -> Self {
        NestedCmd::Bool(false)
    }
}

/// Metadata about the LLM extraction
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ExtractionMeta {
    /// Provider/model identifier (e.g., "openai/gpt-5-mini")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// A processed man page with extracted options
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ParsedManpage {
    /// Source path (e.g., "ubuntu/26.04/1/tar.1.gz")
    pub source: String,
    /// Command name (e.g., "tar")
    pub name: String,
    /// One-line synopsis from the man page
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub synopsis: Option<String>,
    /// Extracted options
    #[serde(default)]
    pub options: Vec<CliOption>,
    /// Aliases with scores [(alias, score)]
    #[serde(default)]
    pub aliases: Vec<(String, i32)>,
    /// Allow matching options without leading dash
    #[serde(default)]
    pub dashless_opts: bool,
    /// Subcommand names (e.g., ["commit", "push", "pull"])
    #[serde(default)]
    pub subcommands: Vec<String>,
    /// Manually updated flag
    #[serde(default)]
    pub updated: bool,
    /// Positional arguments can start a nested command
    #[serde(default)]
    pub nested_cmd: NestedCmd,
    /// Extractor identifier (e.g., "llm")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extractor: Option<String>,
    /// Extraction metadata
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extraction_meta: Option<ExtractionMeta>,
}

impl ParsedManpage {
    /// Get all positional options without prefix, in document order.
    /// Returns (positional-name, merged-help-text) pairs; the Vec preserves
    /// the Python OrderedDict ordering without an extra dependency.
    pub fn positionals(&self) -> Vec<(String, String)> {
        let mut order: Vec<String> = Vec::new();
        let mut groups: std::collections::HashMap<String, Vec<&CliOption>> =
            std::collections::HashMap::new();

        for opt in &self.options {
            if let Some(Positional::Name(name)) = &opt.positional {
                if opt.prefix.is_none() {
                    groups.entry(name.clone()).or_default().push(opt);
                    if !order.contains(name) {
                        order.push(name.clone());
                    }
                }
            }
        }

        order
            .into_iter()
            .map(|name| {
                let opts = &groups[&name];
                let text = opts
                    .iter()
                    .map(|o| o.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n\n");
                (name, text)
            })
            .collect()
    }

    /// Get prefixed positional options in document order.
    /// Returns (positional-name, (prefix, merged-help-text)) entries.
    pub fn prefixed_positionals(&self) -> Vec<(String, (String, String))> {
        let mut order: Vec<String> = Vec::new();
        let mut groups: std::collections::HashMap<String, Vec<&CliOption>> =
            std::collections::HashMap::new();

        for opt in &self.options {
            if let Some(Positional::Name(name)) = &opt.positional {
                if opt.prefix.is_some() {
                    groups.entry(name.clone()).or_default().push(opt);
                    if !order.contains(name) {
                        order.push(name.clone());
                    }
                }
            }
        }

        order
            .into_iter()
            .map(|name| {
                let opts = &groups[&name];
                let prefix = opts[0].prefix.clone().unwrap_or_default();
                let text = opts
                    .iter()
                    .map(|o| o.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n\n");
                (name, (prefix, text))
            })
            .collect()
    }

    /// Find an option by flag name
    pub fn find_option(&self, flag: &str) -> Option<&CliOption> {
        self.options.iter().find(|o| o.matches_flag(flag))
    }
}

/// Result of explaining a command token
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MatchResult {
    /// Start position in input string
    pub start: usize,
    /// End position in input string (exclusive)
    pub end: usize,
    /// Help text from manpage, or null if unknown
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The matched portion of the input string
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_text: Option<String>,
    /// Debug metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub debug_info: Option<serde_json::Value>,
}

impl MatchResult {
    /// Check if this match is unknown (no help text)
    pub fn is_unknown(&self) -> bool {
        self.text.is_none()
    }
}

/// A group of match results (one per command/shell construct)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MatchGroup {
    /// Group name (e.g., "shell", "command1", "command2")
    pub name: String,
    /// Match results in this group
    #[serde(default)]
    pub results: Vec<MatchResult>,
    /// Associated manpage (if any)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manpage: Option<ParsedManpage>,
    /// Alternative manpage suggestions
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suggestions: Vec<ParsedManpage>,
    /// Error if manpage not found
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Positional argument consumption index
    #[serde(default)]
    pub positional_index: usize,
}

/// Complete explanation result
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExplainResult {
    /// All match groups
    pub groups: Vec<MatchGroup>,
    /// Expansions detected during parsing
    #[serde(default)]
    pub expansions: Vec<Expansion>,
}

/// Word expansion information
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Expansion {
    pub start: usize,
    pub end: usize,
    pub kind: String, // "substitution", "process_substitution", "tilde", "parameter-*"
}