//! Shell AST mirroring bashlex node kinds.
//!
//! Field names and shapes follow bashlex (ast.py) so the matcher port can
//! stay close to explainshell/matcher.py: every node carries a byte `span`
//! into the original input, words carry quote-removed text with the raw span
//! (the matcher detects quoting via `span.len() != word.len()`).

use serde::{Deserialize, Serialize};

/// Byte span into the original input string: [start, end).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    /// Start offset (inclusive)
    pub start: usize,
    /// End offset (exclusive)
    pub end: usize,
}

impl Span {
    /// Length in bytes.
    pub fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// True when the span is empty.
    pub fn is_empty(&self) -> bool {
        self.end <= self.start
    }

    /// Shift both ends by `offset` (multi-command parsing).
    pub fn shifted(&self, offset: usize) -> Span {
        Span {
            start: self.start + offset,
            end: self.end + offset,
        }
    }

    /// Minimal span covering all parts.
    pub fn covering(spans: impl Iterator<Item = Span>) -> Option<Span> {
        let mut iter = spans;
        let first = iter.next()?;
        let mut start = first.start;
        let mut end = first.end;
        for s in iter {
            start = start.min(s.start);
            end = end.max(s.end);
        }
        Some(Span { start, end })
    }
}

/// A word token: quote-removed text plus expansion children.
///
/// `text` keeps expansions verbatim (`$(...)`, `$VAR`, `~user`); `span`
/// covers the raw source including quotes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WordNode {
    /// Source span (includes quotes)
    pub span: Span,
    /// Quote-removed, expansion-preserving text
    pub text: String,
    /// Expansion children (substitutions, tilde, parameters)
    pub parts: Vec<AstNode>,
    /// The word was quoted or escaped anywhere
    pub quoted: bool,
}

impl WordNode {
    /// True when the word carries expansions (bashlex: `word_node.parts`).
    pub fn is_expanded(&self) -> bool {
        !self.parts.is_empty()
    }

    /// True when the raw span is longer than the cooked text, i.e. the
    /// word was quoted (bashlex quote-detection idiom used by the matcher).
    pub fn was_quoted(&self) -> bool {
        self.quoted || self.span.len() != self.text.len()
    }
}

/// Redirect target: a word, a file descriptor, a closing dash, or absent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RedirectTarget {
    /// Target word node
    Word(WordNode),
    /// Numeric file descriptor (`2>&1`)
    Fd(i64),
    /// Closing dash (`>&-`, `<&-`)
    Dash,
    /// No target
    None,
}

/// Redirect source: optional fd number or `{varname}` word.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RedirectSource {
    /// Numeric file descriptor (`2>`)
    Fd(i64),
    /// `{varname}` form
    Word(String),
}

/// A shell AST node. Variant names match bashlex `kind` strings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AstNode {
    /// Top-level command list
    List {
        /// Source span
        span: Span,
        /// Children (commands, pipelines, operators)
        parts: Vec<AstNode>,
    },
    /// `cmd1 | cmd2`, optionally negated with leading `!`
    Pipeline {
        /// Source span
        span: Span,
        /// Children (commands, pipes, leading `!` reserved word)
        parts: Vec<AstNode>,
    },
    /// Simple command: program plus arguments and redirects
    Command {
        /// Source span
        span: Span,
        /// Words, redirects, assignments in order
        parts: Vec<AstNode>,
    },
    /// `{ ...; }`, `( ... )`, if/for/while/until/case wrappers
    Compound {
        /// Source span
        span: Span,
        /// Inner command list plus reserved words
        list: Vec<AstNode>,
        /// Trailing redirects (`} >file`)
        redirects: Vec<AstNode>,
    },
    /// if / for / while / until / case / select bodies
    /// (bashlex uses one variant per keyword; the keyword is recoverable
    /// from the first reserved-word part)
    Keyword {
        /// bashlex kind: if, for, while, until, case, pattern
        kind: KeywordKind,
        /// Source span
        span: Span,
        /// Reserved words, lists, words
        parts: Vec<AstNode>,
    },
    /// Function definition
    Function {
        /// Source span
        span: Span,
        /// Function name word
        name: WordNode,
        /// Body compound command
        body: Box<AstNode>,
        /// All parts including reserved words
        parts: Vec<AstNode>,
    },
    /// Plain word
    Word(WordNode),
    /// `NAME=value` prefix word
    Assignment(WordNode),
    /// `;`, `&`, `&&`, `||`
    Operator {
        /// Source span
        span: Span,
        /// Operator text
        op: String,
    },
    /// `|` or `|&`
    Pipe {
        /// Source span
        span: Span,
        /// Pipe text
        pipe: String,
    },
    /// Shell redirection
    Redirect {
        /// Source span
        span: Span,
        /// Optional fd or `{varname}` source
        input: Option<RedirectSource>,
        /// Operator text (`>`, `<`, `>>`, `<<`, `>&`, `<&`, `&>`, `&>>`, `>|`, `<>`, `<<<`, `<<-`)
        op: String,
        /// Target
        output: RedirectTarget,
        /// Heredoc body for `<<` / `<<-`
        heredoc: Option<Box<AstNode>>,
    },
    /// Reserved word (`if`, `then`, `do`, `{`, `!`, ...)
    ReservedWord {
        /// Source span
        span: Span,
        /// Word text
        word: String,
    },
    /// `$(...)` or backquote substitution with parsed command
    CommandSubstitution {
        /// Source span (includes delimiters)
        span: Span,
        /// Parsed inner command
        command: Box<AstNode>,
    },
    /// `<(...)` / `>(...)` with parsed command
    ProcessSubstitution {
        /// Source span (includes delimiters)
        span: Span,
        /// Parsed inner command
        command: Box<AstNode>,
    },
    /// `$name`, `${...}`, `$1`, `$*`, ...
    Parameter {
        /// Source span
        span: Span,
        /// Raw parameter text
        value: String,
    },
    /// `~`, `~/x`, `~user/...`
    Tilde {
        /// Source span
        span: Span,
        /// Raw tilde text
        value: String,
    },
    /// Heredoc body
    Heredoc {
        /// Source span of the body
        span: Span,
        /// Body text
        value: String,
    },
    /// Parsed-but-unsupported construct (bashlex `proceedonerror` mode)
    Unimplemented {
        /// Source span
        span: Span,
        /// Best-effort children
        parts: Vec<AstNode>,
    },
}

/// Keyword-command flavors (bashlex uses one kind per keyword).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeywordKind {
    /// if/then/elif/else/fi
    If,
    /// for/in/do/done (also select)
    For,
    /// while/do/done
    While,
    /// until/do/done
    Until,
    /// case/in/esac
    Case,
    /// case pattern alternative
    Pattern,
}

impl AstNode {
    /// bashlex `kind` string for this node.
    pub fn kind(&self) -> &'static str {
        match self {
            AstNode::List { .. } => "list",
            AstNode::Pipeline { .. } => "pipeline",
            AstNode::Command { .. } => "command",
            AstNode::Compound { .. } => "compound",
            AstNode::Keyword { kind, .. } => match kind {
                KeywordKind::If => "if",
                KeywordKind::For => "for",
                KeywordKind::While => "while",
                KeywordKind::Until => "until",
                KeywordKind::Case => "case",
                KeywordKind::Pattern => "pattern",
            },
            AstNode::Function { .. } => "function",
            AstNode::Word(_) => "word",
            AstNode::Assignment(_) => "assignment",
            AstNode::Operator { .. } => "operator",
            AstNode::Pipe { .. } => "pipe",
            AstNode::Redirect { .. } => "redirect",
            AstNode::ReservedWord { .. } => "reservedword",
            AstNode::CommandSubstitution { .. } => "commandsubstitution",
            AstNode::ProcessSubstitution { .. } => "processsubstitution",
            AstNode::Parameter { .. } => "parameter",
            AstNode::Tilde { .. } => "tilde",
            AstNode::Heredoc { .. } => "heredoc",
            AstNode::Unimplemented { .. } => "unimplemented",
        }
    }

    /// Source span of this node.
    pub fn span(&self) -> Span {
        match self {
            AstNode::List { span, .. }
            | AstNode::Pipeline { span, .. }
            | AstNode::Command { span, .. }
            | AstNode::Compound { span, .. }
            | AstNode::Keyword { span, .. }
            | AstNode::Function { span, .. }
            | AstNode::Operator { span, .. }
            | AstNode::Pipe { span, .. }
            | AstNode::Redirect { span, .. }
            | AstNode::ReservedWord { span, .. }
            | AstNode::CommandSubstitution { span, .. }
            | AstNode::ProcessSubstitution { span, .. }
            | AstNode::Parameter { span, .. }
            | AstNode::Tilde { span, .. }
            | AstNode::Heredoc { span, .. }
            | AstNode::Unimplemented { span, .. } => *span,
            AstNode::Word(w) | AstNode::Assignment(w) => w.span,
        }
    }

    /// Direct children in visit order (bashlex nodevisitor traversal).
    pub fn children(&self) -> Vec<&AstNode> {
        match self {
            AstNode::List { parts, .. }
            | AstNode::Pipeline { parts, .. }
            | AstNode::Command { parts, .. }
            | AstNode::Keyword { parts, .. }
            | AstNode::Function { parts, .. }
            | AstNode::Unimplemented { parts, .. } => parts.iter().collect(),
            AstNode::Compound { list, redirects, .. } => {
                list.iter().chain(redirects.iter()).collect()
            }
            AstNode::Word(w) | AstNode::Assignment(w) => w.parts.iter().collect(),
            AstNode::Redirect { output, heredoc, .. } => {
                let mut out: Vec<&AstNode> = Vec::new();
                if let RedirectTarget::Word(w) = output {
                    out.extend(w.parts.iter());
                }
                if let Some(h) = heredoc {
                    out.push(h);
                }
                out
            }
            AstNode::CommandSubstitution { command, .. }
            | AstNode::ProcessSubstitution { command, .. } => vec![command],
            AstNode::Operator { .. }
            | AstNode::Pipe { .. }
            | AstNode::ReservedWord { .. }
            | AstNode::Parameter { .. }
            | AstNode::Tilde { .. }
            | AstNode::Heredoc { .. } => Vec::new(),
        }
    }

    /// Shift every span in this subtree by `offset` (bashlex posshifter).
    pub fn shifted(&self, offset: usize) -> AstNode {
        if offset == 0 {
            return self.clone();
        }
        match self {
            AstNode::List { span, parts } => AstNode::List {
                span: span.shifted(offset),
                parts: parts.iter().map(|p| p.shifted(offset)).collect(),
            },
            AstNode::Pipeline { span, parts } => AstNode::Pipeline {
                span: span.shifted(offset),
                parts: parts.iter().map(|p| p.shifted(offset)).collect(),
            },
            AstNode::Command { span, parts } => AstNode::Command {
                span: span.shifted(offset),
                parts: parts.iter().map(|p| p.shifted(offset)).collect(),
            },
            AstNode::Compound {
                span,
                list,
                redirects,
            } => AstNode::Compound {
                span: span.shifted(offset),
                list: list.iter().map(|p| p.shifted(offset)).collect(),
                redirects: redirects.iter().map(|p| p.shifted(offset)).collect(),
            },
            AstNode::Keyword { kind, span, parts } => AstNode::Keyword {
                kind: *kind,
                span: span.shifted(offset),
                parts: parts.iter().map(|p| p.shifted(offset)).collect(),
            },
            AstNode::Function {
                span,
                name,
                body,
                parts,
            } => AstNode::Function {
                span: span.shifted(offset),
                name: WordNode {
                    span: name.span.shifted(offset),
                    text: name.text.clone(),
                    parts: name.parts.iter().map(|p| p.shifted(offset)).collect(),
                    quoted: name.quoted,
                },
                body: Box::new(body.shifted(offset)),
                parts: parts.iter().map(|p| p.shifted(offset)).collect(),
            },
            AstNode::Word(w) | AstNode::Assignment(w) => {
                let shifted_word = WordNode {
                    span: w.span.shifted(offset),
                    text: w.text.clone(),
                    parts: w.parts.iter().map(|p| p.shifted(offset)).collect(),
                    quoted: w.quoted,
                };
                if matches!(self, AstNode::Assignment(_)) {
                    AstNode::Assignment(shifted_word)
                } else {
                    AstNode::Word(shifted_word)
                }
            }
            AstNode::Operator { span, op } => AstNode::Operator {
                span: span.shifted(offset),
                op: op.clone(),
            },
            AstNode::Pipe { span, pipe } => AstNode::Pipe {
                span: span.shifted(offset),
                pipe: pipe.clone(),
            },
            AstNode::Redirect {
                span,
                input,
                op,
                output,
                heredoc,
            } => AstNode::Redirect {
                span: span.shifted(offset),
                input: input.clone(),
                op: op.clone(),
                output: match output {
                    RedirectTarget::Word(w) => RedirectTarget::Word(WordNode {
                        span: w.span.shifted(offset),
                        text: w.text.clone(),
                        parts: w.parts.iter().map(|p| p.shifted(offset)).collect(),
                        quoted: w.quoted,
                    }),
                    other => other.clone(),
                },
                heredoc: heredoc
                    .as_ref()
                    .map(|h| Box::new(h.shifted(offset))),
            },
            AstNode::ReservedWord { span, word } => AstNode::ReservedWord {
                span: span.shifted(offset),
                word: word.clone(),
            },
            AstNode::CommandSubstitution { span, command } => {
                AstNode::CommandSubstitution {
                    span: span.shifted(offset),
                    command: Box::new(command.shifted(offset)),
                }
            }
            AstNode::ProcessSubstitution { span, command } => {
                AstNode::ProcessSubstitution {
                    span: span.shifted(offset),
                    command: Box::new(command.shifted(offset)),
                }
            }
            AstNode::Parameter { span, value } => AstNode::Parameter {
                span: span.shifted(offset),
                value: value.clone(),
            },
            AstNode::Tilde { span, value } => AstNode::Tilde {
                span: span.shifted(offset),
                value: value.clone(),
            },
            AstNode::Heredoc { span, value } => AstNode::Heredoc {
                span: span.shifted(offset),
                value: value.clone(),
            },
            AstNode::Unimplemented { span, parts } => AstNode::Unimplemented {
                span: span.shifted(offset),
                parts: parts.iter().map(|p| p.shifted(offset)).collect(),
            },
        }
    }

    /// Maximum end offset in this subtree, counting heredoc bodies
    /// (bashlex _endfinder: heredocs may extend past the node end).
    pub fn subtree_end(&self) -> usize {
        let mut end = self.span().end;
        let mut stack = self.children();
        while let Some(child) = stack.pop() {
            end = end.max(child.span().end);
            stack.extend(child.children());
        }
        end
    }
}

/// Index of the first part with the given bashlex kind, or -1
/// (bashlex findfirstkind).
pub fn find_first_kind(parts: &[AstNode], kind: &str) -> i64 {
    parts
        .iter()
        .position(|p| p.kind() == kind)
        .map(|i| i as i64)
        .unwrap_or(-1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(text: &str, start: usize, end: usize) -> AstNode {
        AstNode::Word(WordNode {
            span: Span { start, end },
            text: text.to_string(),
            parts: Vec::new(),
            quoted: false,
        })
    }

    #[test]
    fn kinds_match_bashlex() {
        assert_eq!(word("x", 0, 1).kind(), "word");
        assert_eq!(
            AstNode::Operator {
                span: Span { start: 0, end: 2 },
                op: "&&".to_string()
            }
            .kind(),
            "operator"
        );
        assert_eq!(
            AstNode::Keyword {
                kind: KeywordKind::For,
                span: Span { start: 0, end: 3 },
                parts: Vec::new()
            }
            .kind(),
            "for"
        );
    }

    #[test]
    fn find_first_kind_behaves_like_bashlex() {
        let parts = vec![
            word("echo", 0, 4),
            AstNode::ReservedWord {
                span: Span { start: 5, end: 7 },
                word: "do".to_string(),
            },
            word("x", 8, 9),
        ];
        assert_eq!(find_first_kind(&parts, "word"), 0);
        assert_eq!(find_first_kind(&parts, "reservedword"), 1);
        assert_eq!(find_first_kind(&parts, "pipe"), -1);
    }

    #[test]
    fn shift_moves_whole_subtree() {
        let cmd = AstNode::Command {
            span: Span { start: 0, end: 8 },
            parts: vec![word("echo", 0, 4), word("hi", 5, 7)],
        };
        let moved = cmd.shifted(10);
        assert_eq!(moved.span(), Span { start: 10, end: 18 });
        assert_eq!(moved.subtree_end(), 18);
        // Zero shift clones without touching anything.
        assert_eq!(cmd.shifted(0), cmd);
    }

    #[test]
    fn quote_detection_matches_matcher_idiom() {
        let quoted = WordNode {
            span: Span { start: 0, end: 7 },
            text: "x".to_string(),
            parts: Vec::new(),
            quoted: true,
        };
        assert!(quoted.was_quoted());
        let plain = WordNode {
            span: Span { start: 0, end: 1 },
            text: "x".to_string(),
            parts: Vec::new(),
            quoted: false,
        };
        assert!(!plain.was_quoted());
    }

    #[test]
    fn subtree_end_counts_heredoc_body() {
        let redir = AstNode::Redirect {
            span: Span { start: 0, end: 10 },
            input: None,
            op: "<<".to_string(),
            output: RedirectTarget::Word(WordNode {
                span: Span { start: 3, end: 6 },
                text: "EOF".to_string(),
                parts: Vec::new(),
                quoted: false,
            }),
            heredoc: Some(Box::new(AstNode::Heredoc {
                span: Span { start: 11, end: 20 },
                value: "body\n".to_string(),
            })),
        };
        assert_eq!(redir.subtree_end(), 20);
    }
}
