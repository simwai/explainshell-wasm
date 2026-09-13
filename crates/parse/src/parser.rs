//! Recursive-descent shell parser producing bashlex-compatible AST.
//!
//! Grammar coverage mirrors the bashlex productions the explainshell
//! matcher depends on: simple commands, pipelines, lists, redirects
//! (including heredocs), if/for/while/until/case compounds, functions,
//! subshells, and groups. Deliberate divergences from bashlex:
//!
//! - Bare newlines separate commands without emitting `\n` operator nodes
//!   (bashlex emits them; explainshell's matcher would crash on the
//!   lookup). `;`, `&`, `&&`, `||` still become operator nodes.
//! - `select` parses exactly like `for` (bashlex raises NotImplemented).
//! - `time` / `coproc` wrap the following pipeline in an Unimplemented
//!   node instead of raising.
//! - Stray reserved words in argument position degrade to unknown words
//!   instead of raising.
//! - `[[ ... ]]` degrades to a plain command (unknown program) instead of
//!   raising; `((...))` degrades to nested subshells.
//! - Positions are byte offsets; callers slicing non-ASCII input must
//!   account for UTF-8 widths.

use crate::ast::{AstNode, KeywordKind, RedirectSource, RedirectTarget, Span, WordNode};
use crate::lexer::{LexError, Lexer, OpKind, RawExpansion, RawWord, Token};

/// Parser failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Lexer failure
    Lex(LexError),
    /// Unexpected token where a command could start
    Unexpected {
        /// Description of what was found
        found: String,
        /// Byte offset
        offset: usize,
    },
    /// Unexpected end of input
    UnexpectedEof,
    /// No command in the input
    EmptyInput,
    /// Construct parsed but explicitly unsupported
    Unimplemented {
        /// Construct name
        what: String,
        /// Byte offset
        offset: usize,
    },
}

impl ParseError {
    /// Byte offset of the failure, when known.
    pub fn offset(&self) -> Option<usize> {
        match self {
            ParseError::Lex(e) => Some(e.offset),
            ParseError::Unexpected { offset, .. } => Some(*offset),
            ParseError::Unimplemented { offset, .. } => Some(*offset),
            ParseError::UnexpectedEof | ParseError::EmptyInput => None,
        }
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Lex(e) => write!(f, "lex error: {e}"),
            ParseError::Unexpected { found, offset } => {
                write!(f, "unexpected {found} at offset {offset}")
            }
            ParseError::UnexpectedEof => write!(f, "unexpected end of input"),
            ParseError::EmptyInput => write!(f, "no command to parse"),
            ParseError::Unimplemented { what, offset } => {
                write!(f, "unsupported construct {what} at offset {offset}")
            }
        }
    }
}

impl std::error::Error for ParseError {}

impl From<LexError> for ParseError {
    fn from(e: LexError) -> ParseError {
        ParseError::Lex(e)
    }
}

/// Parse options.
#[derive(Debug, Clone, Copy)]
pub struct ParseOptions {
    /// Gather heredoc bodies strictly (error on missing delimiter).
    /// Matches bashlex `strictmode`.
    pub strict: bool,
    /// Maximum substitution nesting that keeps child nodes.
    /// Matches bashlex `expansionlimit`. `None` means unbounded.
    pub expansion_limit: Option<i32>,
}

impl Default for ParseOptions {
    fn default() -> ParseOptions {
        ParseOptions {
            strict: true,
            expansion_limit: None,
        }
    }
}

/// Parse a single top-level command, ignoring anything after the first
/// terminator (bashlex `parsesingle` + accept semantics).
pub fn parsesingle(
    input: &str,
    strict: bool,
    expansion_limit: Option<i32>,
) -> Result<AstNode, ParseError> {
    let mut p = Parser::new(input, strict, expansion_limit, 0);
    p.skip_newlines()?;
    match p.peek(0)? {
        Token::Eof => Err(ParseError::EmptyInput),
        _ => p.parse_command_sequence(&EndSet::top()),
    }
}

/// Parse the whole input into top-level nodes (bashlex `parse`).
pub fn parse(
    input: &str,
    strict: bool,
    expansion_limit: Option<i32>,
) -> Result<Vec<AstNode>, ParseError> {
    let mut p = Parser::new(input, strict, expansion_limit, 0);
    let mut parts = Vec::new();
    loop {
        p.skip_newlines()?;
        if matches!(p.peek(0)?, Token::Eof) {
            break;
        }
        let before = p.lexer_offset();
        let node = p.parse_command_sequence(&EndSet::top())?;
        let resume = node.span().end.max(node.subtree_end());
        p.skip_to(resume);
        parts.push(node);
        if matches!(p.peek(0)?, Token::Eof) {
            break;
        }
        // Progress guard: never spin on unconsumed input.
        if p.lexer_offset() <= before {
            return Err(ParseError::Unexpected {
                found: "unparsed input".to_string(),
                offset: before,
            });
        }
    }
    Ok(parts)
}

/// What terminates a command sequence.
#[derive(Debug, Clone, Copy)]
struct EndSet {
    /// Stop before `)` (subshells, substitutions)
    stop_rparen: bool,
    /// Stop before `}` (groups, for-brace form)
    stop_rbrace: bool,
    /// Stop before these command-head words (`done`, `fi`, ...)
    stop_words: &'static [&'static str],
}

impl EndSet {
    fn top() -> EndSet {
        EndSet {
            stop_rparen: false,
            stop_rbrace: false,
            stop_words: &[],
        }
    }
}

/// Recursive-descent parser over a streaming lexer.
struct Parser<'a> {
    input: &'a [u8],
    lexer: Lexer<'a>,
    /// Lookahead buffer (at most two tokens).
    buf: Vec<Token>,
    /// Current substitution nesting depth.
    depth: i32,
    /// Depth limit for substitution children (`i32::MAX` when unbounded).
    limit: i32,
    strict: bool,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str, strict: bool, expansion_limit: Option<i32>, depth: i32) -> Parser<'a> {
        Parser {
            input: input.as_bytes(),
            lexer: Lexer::new(input),
            buf: Vec::new(),
            depth,
            limit: expansion_limit.unwrap_or(i32::MAX),
            strict,
        }
    }

    fn lexer_offset(&self) -> usize {
        self.lexer.offset()
    }

    /// Drop buffered tokens at or past `off` and jump the lexer there.
    fn skip_to(&mut self, off: usize) {
        self.lexer.skip_to(off);
        self.buf.retain(|t| match t.span() {
            Some(s) => s.start >= off,
            None => true,
        });
    }

    fn fill(&mut self) -> Result<(), ParseError> {
        while self.buf.len() < 2 {
            let tok = self.lexer.next_token()?;
            let done = matches!(tok, Token::Eof);
            self.buf.push(tok);
            if done {
                break;
            }
        }
        Ok(())
    }

    fn peek(&mut self, i: usize) -> Result<&Token, ParseError> {
        while self.buf.len() <= i {
            let tok = self.lexer.next_token()?;
            let done = matches!(tok, Token::Eof);
            self.buf.push(tok);
            if done {
                break;
            }
        }
        // The buffer always ends with Eof once the lexer is exhausted.
        let idx = self.buf.len().min(i);
        Ok(&self.buf[idx])
    }

    fn next(&mut self) -> Result<Token, ParseError> {
        self.fill()?;
        if matches!(self.buf.first(), Some(Token::Eof)) {
            return Ok(Token::Eof);
        }
        Ok(self.buf.remove(0))
    }

    fn skip_newlines(&mut self) -> Result<(), ParseError> {
        loop {
            match self.peek(0)? {
                Token::Newline(_) => {
                    self.next()?;
                }
                _ => break,
            }
        }
        Ok(())
    }

    fn token_desc(tok: &Token) -> String {
        match tok {
            Token::Word(w) => format!("word {:?}", w.cooked),
            Token::Newline(_) => "newline".to_string(),
            Token::Op(k, _) => format!("operator {:?}", k.text()),
            Token::Eof => "end of input".to_string(),
        }
    }

    fn peek_offset(&mut self) -> Result<usize, ParseError> {
        Ok(self.peek(0)?.span().map(|s| s.start).unwrap_or(self.input.len()))
    }

    /// True when the lookahead can start a command.
    fn at_command_start(&mut self) -> Result<bool, ParseError> {
        Ok(match self.peek(0)? {
            Token::Word(_) => true,
            Token::Op(k, _) => matches!(
                k,
                OpKind::LParen
                    | OpKind::LBrace
                    | OpKind::Bang
                    | OpKind::Less
                    | OpKind::Greater
                    | OpKind::GreaterGreater
                    | OpKind::LessLess
                    | OpKind::LessLessMinus
                    | OpKind::LessLessLess
                    | OpKind::LessAnd
                    | OpKind::GreaterAnd
                    | OpKind::AndGreater
                    | OpKind::AndGreaterGreater
                    | OpKind::LessGreater
                    | OpKind::GreaterBar
            ),
            Token::Newline(_) | Token::Eof => false,
        })
    }

    /// Parse `pipeline [(&&|||&|;) pipeline]*` with bashlex list semantics.
    fn parse_command_sequence(&mut self, end: &EndSet) -> Result<AstNode, ParseError> {
        let mut parts = vec![self.parse_pipeline(end)?];
        loop {
            self.skip_newlines()?;
            if self.at_end(end)? {
                break;
            }
            match self.peek(0)?.clone() {
                Token::Op(OpKind::AndAnd, span) | Token::Op(OpKind::OrOr, span) => {
                    self.next()?;
                    self.skip_newlines()?;
                    if !self.at_command_start()? {
                        return Err(ParseError::Unexpected {
                            found: "end of input after operator".to_string(),
                            offset: span.end,
                        });
                    }
                    parts.push(AstNode::Operator {
                        span,
                        op: self.op_text(&span),
                    });
                    parts.push(self.parse_pipeline(end)?);
                }
                Token::Op(OpKind::Amp, span) | Token::Op(OpKind::Semi, span) => {
                    self.next()?;
                    self.skip_newlines()?;
                    if self.at_stop_word(end)? {
                        break;
                    }
                    parts.push(AstNode::Operator {
                        span,
                        op: self.op_text(&span),
                    });
                    if !self.at_end(end)? && self.at_command_start()? {
                        parts.push(self.parse_pipeline(end)?);
                    } else {
                        break;
                    }
                }
                _ => break,
            }
        }
        if parts.len() == 1 {
            Ok(parts.into_iter().next().unwrap_or_else(|| unreachable!()))
        } else {
            let span = Span::covering(parts.iter().map(|p| p.span())).unwrap_or(Span {
                start: 0,
                end: 0,
            });
            Ok(AstNode::List { span, parts })
        }
    }

    /// Operator text from the input bytes (avoids cloning through peek).
    fn op_text(&self, span: &Span) -> String {
        String::from_utf8_lossy(&self.input[span.start..span.end]).into_owned()
    }

    /// True when the lookahead ends the current sequence.
    fn at_end(&mut self, end: &EndSet) -> Result<bool, ParseError> {
        Ok(match self.peek(0)? {
            Token::Eof | Token::Newline(_) => true,
            Token::Op(OpKind::RParen, _) => end.stop_rparen,
            Token::Op(OpKind::RBrace, _) => end.stop_rbrace,
            Token::Op(OpKind::SemiSemi, _)
            | Token::Op(OpKind::SemiAnd, _)
            | Token::Op(OpKind::SemiSemiAnd, _) => true,
            Token::Word(w) => end.stop_words.iter().any(|s| *s == w.cooked),
            _ => false,
        })
    }

    /// True when the current token is a stop word for the current end set.
    fn at_stop_word(&mut self, end: &EndSet) -> Result<bool, ParseError> {
        Ok(match self.peek(0)? {
            Token::Word(w) => end.stop_words.iter().any(|s| *s == w.cooked),
            _ => false,
        })
    }

    /// Parse `[!] command (| command)*`.
    fn parse_pipeline(&mut self, end: &EndSet) -> Result<AstNode, ParseError> {
        let mut parts = Vec::new();
        if matches!(self.peek(0)?, Token::Op(OpKind::Bang, _)) {
            let span = self.peek(0)?.span().unwrap_or(Span { start: 0, end: 0 });
            self.next()?;
            parts.push(AstNode::ReservedWord {
                span,
                word: "!".to_string(),
            });
        }
        parts.push(self.parse_command_unit(end)?);
        loop {
            match self.peek(0)? {
                Token::Op(OpKind::Pipe, span) | Token::Op(OpKind::PipeAmp, span) => {
                    let span = *span;
                    self.next()?;
                    parts.push(AstNode::Pipe {
                        span,
                        pipe: self.op_text(&span),
                    });
                    parts.push(self.parse_command_unit(end)?);
                }
                _ => break,
            }
        }
        if parts.len() == 1 {
            Ok(parts.into_iter().next().unwrap_or_else(|| unreachable!()))
        } else {
            let span = Span::covering(parts.iter().map(|p| p.span())).unwrap_or(Span {
                start: 0,
                end: 0,
            });
            Ok(AstNode::Pipeline { span, parts })
        }
    }

    /// Parse one command: compound, function, subshell, group, or simple.
    fn parse_command_unit(&mut self, end: &EndSet) -> Result<AstNode, ParseError> {
        // Stop-words and closers never start a unit; callers check first,
        // but `&&`/`||` right-hand sides land here directly.
        match self.peek(0)?.clone() {
            Token::Word(w) if end.stop_words.iter().any(|s| *s == w.cooked) => {
                return Err(ParseError::Unexpected {
                    found: format!("word {:?}", w.cooked),
                    offset: w.span.start,
                })
            }
            Token::Op(OpKind::RParen, _) if end.stop_rparen => {
                return Err(ParseError::Unexpected {
                    found: "operator \")\"".to_string(),
                    offset: self.peek_offset()?,
                })
            }
            Token::Op(OpKind::RBrace, _) if end.stop_rbrace => {
                return Err(ParseError::Unexpected {
                    found: "operator \"}\"".to_string(),
                    offset: self.peek_offset()?,
                })
            }
            _ => {}
        }
        match self.peek(0)?.clone() {
            Token::Word(w) => {
                if w.cooked == "if" {
                    self.parse_if()
                } else if w.cooked == "for" || w.cooked == "select" {
                    self.parse_for()
                } else if w.cooked == "while" {
                    self.parse_loop(KeywordKind::While, "while")
                } else if w.cooked == "until" {
                    self.parse_loop(KeywordKind::Until, "until")
                } else if w.cooked == "case" {
                    self.parse_case()
                } else if w.cooked == "function" {
                    self.parse_function()
                } else if w.cooked == "time" || w.cooked == "coproc" {
                    self.parse_unimplemented_prefix()
                } else if self.peek_is_function_def()? {
                    self.parse_function()
                } else {
                    self.parse_simple_command()
                }
            }
            Token::Op(OpKind::LParen, _) => self.parse_subshell(),
            Token::Op(OpKind::LBrace, _) => self.parse_group(),
            other => Err(ParseError::Unexpected {
                found: Self::token_desc(&other),
                offset: self.peek_offset()?,
            }),
        }
    }

    /// True when `WORD (`... (function definition) follows.
    fn peek_is_function_def(&mut self) -> Result<bool, ParseError> {
        let first = self.peek(0)?.clone();
        let second = self.peek(1)?.clone();
        Ok(match (first, second) {
            (Token::Word(_), Token::Op(OpKind::LParen, _)) => true,
            _ => false,
        })
    }

    /// Parse a simple command: words, assignments, and redirects.
    fn parse_simple_command(&mut self) -> Result<AstNode, ParseError> {
        let mut parts = Vec::new();
        let mut at_start = true;
        loop {
            match self.peek(0)?.clone() {
                Token::Word(w) => {
                    if self.peek_is_redirect_input(&w)? {
                        parts.push(self.parse_redirect()?);
                    } else if at_start && is_assignment(&w, &self.input) {
                        parts.push(AstNode::Assignment(self.build_word(&w)?));
                    } else {
                        parts.push(AstNode::Word(self.build_word(&w)?));
                        at_start = false;
                    }
                    // consume the word (parse_redirect consumed its own)
                    if !matches!(
                        parts.last(),
                        Some(AstNode::Word(_)) | Some(AstNode::Assignment(_))
                    ) {
                        // redirect path already advanced past the word
                    } else {
                        self.next()?;
                    }
                }
                Token::Op(kind, _) if is_redirect_op(kind) => {
                    parts.push(self.parse_redirect()?);
                }
                _ => break,
            }
        }
        if parts.is_empty() {
            return Err(ParseError::Unexpected {
                found: Self::token_desc(self.peek(0)?),
                offset: self.peek_offset()?,
            });
        }
        let span = Span::covering(parts.iter().map(|p| p.span())).unwrap_or(Span {
            start: 0,
            end: 0,
        });
        Ok(AstNode::Command { span, parts })
    }

    /// True when a word prefixes a redirect (`2>`, `{fd}>`).
    fn peek_is_redirect_input(&mut self, w: &RawWord) -> Result<bool, ParseError> {
        if !(is_all_digits(&w.cooked) || is_redir_var(&w.cooked)) {
            return Ok(false);
        }
        Ok(match self.peek(1)? {
            Token::Op(kind, span) => is_redirect_op(*kind) && w.span.end == span.start,
            _ => false,
        })
    }

    /// Parse `[fd|{var}] op target` (heredocs gathered inline).
    fn parse_redirect(&mut self) -> Result<AstNode, ParseError> {
        let mut start = self.peek_offset()?;
        let input = match (self.peek(0)?.clone(), self.peek(1)?.clone()) {
            (Token::Word(w), Token::Op(kind, ospan))
                if is_redirect_op(kind)
                    && w.span.end == ospan.start
                    && (is_all_digits(&w.cooked) || is_redir_var(&w.cooked)) =>
            {
                start = w.span.start;
                self.next()?;
                if is_all_digits(&w.cooked) {
                    Some(RedirectSource::Fd(w.cooked.parse().unwrap_or(0)))
                } else {
                    Some(RedirectSource::Word(w.cooked.clone()))
                }
            }
            _ => None,
        };
        let (opkind, opspan) = match self.next()? {
            Token::Op(kind, span) if is_redirect_op(kind) => (kind, span),
            other => {
                return Err(ParseError::Unexpected {
                    found: Self::token_desc(&other),
                    offset: self.peek_offset()?,
                })
            }
        };
        let op = opkind.text().to_string();

        // `>&-` / `<&-`: closing dash.
        if matches!(opkind, OpKind::LessAnd | OpKind::GreaterAnd) {
            if let Token::Word(w) = self.peek(0)?.clone() {
                if w.cooked == "-" && self.adjacent_to(&opspan, &w.span) {
                    self.next()?;
                    return Ok(AstNode::Redirect {
                        span: Span {
                            start,
                            end: w.span.end,
                        },
                        input,
                        op,
                        output: RedirectTarget::Dash,
                        heredoc: None,
                    });
                }
            }
        }

        let target = match self.next()? {
            Token::Word(w) => {
                if matches!(opkind, OpKind::LessAnd | OpKind::GreaterAnd) && is_all_digits(&w.cooked)
                {
                    RedirectTarget::Fd(w.cooked.parse().unwrap_or(0))
                } else {
                    RedirectTarget::Word(self.build_word(&w)?)
                }
            }
            other => {
                return Err(ParseError::Unexpected {
                    found: Self::token_desc(&other),
                    offset: self.peek_offset()?,
                })
            }
        };
        let mut end = match &target {
            RedirectTarget::Word(w) => w.span.end,
            _ => opspan.end,
        };
        // Fd targets end at their own span; recover it from the lexer offset.
        if !matches!(target, RedirectTarget::Word(_)) {
            end = self.lexer_offset();
        }

        let mut heredoc = None;
        if matches!(opkind, OpKind::LessLess | OpKind::LessLessMinus) {
            if let RedirectTarget::Word(w) = &target {
                let delim = w.text.clone();
                let strip = matches!(opkind, OpKind::LessLessMinus);
                let off = w.span.end;
                match self.gather_heredoc(&delim, strip, off)? {
                    Some((node, resume)) => {
                        heredoc = Some(Box::new(node));
                        self.skip_to(resume);
                    }
                    None => {}
                }
            }
        }

        Ok(AstNode::Redirect {
            span: Span { start, end },
            input,
            op,
            output: target,
            heredoc,
        })
    }

    /// Two spans touch with no gap.
    fn adjacent_to(&self, left: &Span, right: &Span) -> bool {
        left.end == right.start
    }

    /// Gather a heredoc body starting at `off` (the byte right after the
    /// delimiter word). Returns the node plus the offset to resume lexing at.
    fn gather_heredoc(&self, delim: &str, strip_tabs: bool, off: usize) -> Result<Option<(AstNode, usize)>, ParseError> {
        let bytes = self.input;
        // Skip to the end of the delimiter line.
        let line_end = match bytes[off..].iter().position(|&c| c == b'\n') {
            Some(i) => off + i,
            None => {
                if self.strict {
                    return Err(ParseError::UnexpectedEof);
                }
                return Ok(None);
            }
        };
        let body_start = line_end + 1;
        let mut pos = body_start;
        loop {
            if pos >= bytes.len() {
                if self.strict {
                    return Err(ParseError::UnexpectedEof);
                }
                return Ok(None);
            }
            let eol = match bytes[pos..].iter().position(|&c| c == b'\n') {
                Some(i) => pos + i,
                None => bytes.len(),
            };
            let mut line = &bytes[pos..eol];
            if strip_tabs {
                while line.first() == Some(&b'\t') {
                    line = &line[1..];
                }
            }
            if line == delim.as_bytes() {
                let node = AstNode::Heredoc {
                    span: Span {
                        start: body_start,
                        end: eol,
                    },
                    value: String::from_utf8_lossy(&bytes[body_start..pos]).into_owned(),
                };
                return Ok(Some((node, eol)));
            }
            pos = if eol < bytes.len() { eol + 1 } else { eol };
        }
    }

    /// Convert a raw word to an AST word, parsing substitution children
    /// while the nesting depth allows (bashlex expansionlimit semantics).
    fn build_word(&self, raw: &RawWord) -> Result<WordNode, ParseError> {
        let mut parts = Vec::new();
        for exp in &raw.expansions {
            match exp {
                RawExpansion::CmdSubst { span, inner } => {
                    if self.depth < self.limit {
                        let text = self.slice_str(inner)?;
                        let command = self.parse_inner(text, inner.start)?;
                        parts.push(AstNode::CommandSubstitution {
                            span: *span,
                            command: Box::new(command),
                        });
                    }
                }
                RawExpansion::ProcSubst { span, inner } => {
                    if self.depth < self.limit {
                        let text = self.slice_str(inner)?;
                        let command = self.parse_inner(text, inner.start)?;
                        parts.push(AstNode::ProcessSubstitution {
                            span: *span,
                            command: Box::new(command),
                        });
                    }
                }
                RawExpansion::Param { span } => {
                    parts.push(AstNode::Parameter {
                        span: *span,
                        value: String::from_utf8_lossy(&self.input[span.start..span.end])
                            .into_owned(),
                    });
                }
                RawExpansion::Tilde { span } => {
                    parts.push(AstNode::Tilde {
                        span: *span,
                        value: String::from_utf8_lossy(&self.input[span.start..span.end])
                            .into_owned(),
                    });
                }
            }
        }
        Ok(WordNode {
            span: raw.span,
            text: raw.cooked.clone(),
            parts,
            quoted: raw.quoted,
        })
    }

    /// Slice the input as UTF-8.
    fn slice_str(&self, span: &Span) -> Result<&str, ParseError> {
        std::str::from_utf8(&self.input[span.start..span.end]).map_err(|_| {
            ParseError::Unexpected {
                found: "non-utf8 input".to_string(),
                offset: span.start,
            }
        })
    }

    /// Parse substitution content (positions shift back to the outer input).
    fn parse_inner(&self, text: &str, base: usize) -> Result<AstNode, ParseError> {
        let mut sub = Parser::new(text, self.strict, Some(self.limit), self.depth + 1);
        sub.skip_newlines()?;
        if matches!(sub.peek(0)?, Token::Eof) {
            return Err(ParseError::EmptyInput);
        }
        let node = sub.parse_command_sequence(&EndSet::top())?;
        Ok(node.shifted(base))
    }

    /// Consume a reserved word token (`if`, `then`, ...).
    fn reserved(&mut self, word: &str) -> Result<AstNode, ParseError> {
        match self.next()? {
            Token::Word(w) if w.cooked == word => Ok(AstNode::ReservedWord {
                span: w.span,
                word: w.cooked.clone(),
            }),
            other => Err(ParseError::Unexpected {
                found: Self::token_desc(&other),
                offset: self.peek_offset()?,
            }),
        }
    }

    /// Parse `if ...; then ...; [elif ...; then ...;] [else ...;] fi`.
    fn parse_if(&mut self) -> Result<AstNode, ParseError> {
        let start = self.peek_offset()?;
        let mut parts = vec![self.reserved("if")?];
        self.skip_newlines()?;
        parts.push(self.parse_command_sequence(&EndSet {
            stop_words: &["then"],
            ..EndSet::top()
        })?);
        loop {
            self.skip_newlines()?;
            match self.peek(0)?.clone() {
                Token::Word(w) if w.cooked == "then" => {
                    parts.push(self.reserved("then")?);
                    self.skip_newlines()?;
                    parts.push(self.parse_command_sequence(&EndSet {
                        stop_words: &["elif", "else", "fi"],
                        ..EndSet::top()
                    })?);
                }
                Token::Word(w) if w.cooked == "elif" => {
                    parts.push(self.reserved("elif")?);
                    self.skip_newlines()?;
                    parts.push(self.parse_command_sequence(&EndSet {
                        stop_words: &["then"],
                        ..EndSet::top()
                    })?);
                }
                Token::Word(w) if w.cooked == "else" => {
                    parts.push(self.reserved("else")?);
                    self.skip_newlines()?;
                    parts.push(self.parse_command_sequence(&EndSet {
                        stop_words: &["fi"],
                        ..EndSet::top()
                    })?);
                }
                Token::Word(w) if w.cooked == "fi" => {
                    parts.push(self.reserved("fi")?);
                    break;
                }
                other => {
                    return Err(ParseError::Unexpected {
                        found: Self::token_desc(&other),
                        offset: self.peek_offset()?,
                    })
                }
            }
        }
        let end = self.lexer_offset();
        Ok(self.keyword(KeywordKind::If, start, end, parts))
    }

    /// Parse `for name [;] [in items ...] <terminator> do ... done`.
    /// `select` shares this shape.
    fn parse_for(&mut self) -> Result<AstNode, ParseError> {
        let start = self.peek_offset()?;
        let keyword = self.reserved_any(&["for", "select"])?;
        self.skip_newlines()?;
        // Arithmetic for: `for ((...))` degrades to an unimplemented node.
        if matches!(self.peek(0)?, Token::Op(OpKind::LParen, _)) {
            return self.parse_unimplemented_balanced(&keyword, "arithmetic for");
        }
        let name = match self.next()? {
            Token::Word(w) => AstNode::Word(self.build_word(&w)?),
            other => {
                return Err(ParseError::Unexpected {
                    found: Self::token_desc(&other),
                    offset: self.peek_offset()?,
                })
            }
        };
        let mut parts = vec![keyword, name];
        // A `;` directly after the variable is a reserved word.
        if matches!(self.peek(0)?, Token::Op(OpKind::Semi, _)) {
            let span = self.peek(0)?.span().unwrap_or(Span { start, end: start });
            self.next()?;
            parts.push(AstNode::ReservedWord {
                span,
                word: ";".to_string(),
            });
        }
        self.skip_newlines()?;
        // Optional `in` item list; every word (even `do`) is an item, and a
        // `;` terminator stays an operator node (bashlex parity).
        if let Token::Word(w) = self.peek(0)?.clone() {
            if w.cooked == "in" {
                parts.push(self.reserved("in")?);
                loop {
                    match self.peek(0)?.clone() {
                        Token::Word(w) => {
                            let node = AstNode::Word(self.build_word(&w)?);
                            self.next()?;
                            parts.push(node);
                        }
                        Token::Op(OpKind::Semi, span) => {
                            self.next()?;
                            parts.push(AstNode::ReservedWord {
                                span,
                                word: ";".to_string(),
                            });
                            break;
                        }
                        Token::Newline(_) | Token::Eof => break,
                        _ => break,
                    }
                }
            }
        }
        self.skip_newlines()?;
        parts.push(self.reserved_any(&["do", "{"])?);
        let brace = matches!(parts.last(), Some(AstNode::ReservedWord { word, .. }) if word == "{");
        self.skip_newlines()?;
        if brace {
            parts.push(self.parse_command_sequence(&EndSet {
                stop_rbrace: true,
                stop_words: &[],
                ..EndSet::top()
            })?);
            self.skip_newlines()?;
            parts.push(self.reserved("}")?);
        } else {
            parts.push(self.parse_command_sequence(&EndSet {
                stop_words: &["done"],
                ..EndSet::top()
            })?);
            self.skip_newlines()?;
            parts.push(self.reserved("done")?);
        }
        let end = self.lexer_offset();
        Ok(self.keyword(KeywordKind::For, start, end, parts))
    }

    /// Parse `while ...; do ...; done` / `until ...`.
    fn parse_loop(&mut self, kind: KeywordKind, word: &'static str) -> Result<AstNode, ParseError> {
        let start = self.peek_offset()?;
        let mut parts = vec![self.reserved(word)?];
        self.skip_newlines()?;
        parts.push(self.parse_command_sequence(&EndSet {
            stop_words: &["do"],
            ..EndSet::top()
        })?);
        self.skip_newlines()?;
        parts.push(self.reserved("do")?);
        self.skip_newlines()?;
        parts.push(self.parse_command_sequence(&EndSet {
            stop_words: &["done"],
            ..EndSet::top()
        })?);
        self.skip_newlines()?;
        parts.push(self.reserved("done")?);
        let end = self.lexer_offset();
        Ok(self.keyword(kind, start, end, parts))
    }

    /// Parse `case word in [(]pattern) body ;; ... esac`.
    fn parse_case(&mut self) -> Result<AstNode, ParseError> {
        let start = self.peek_offset()?;
        let mut parts = vec![self.reserved("case")?];
        self.skip_newlines()?;
        let subject = self.expect_word()?;
        parts.push(AstNode::Word(self.build_word(&subject)?));
        self.skip_newlines()?;
        parts.push(self.reserved("in")?);
        loop {
            self.skip_newlines()?;
            match self.peek(0)?.clone() {
                Token::Word(w) if w.cooked == "esac" => {
                    parts.push(self.reserved("esac")?);
                    break;
                }
                Token::Eof => {
                    return Err(ParseError::UnexpectedEof);
                }
                _ => {
                    parts.push(self.parse_case_clause()?);
                }
            }
        }
        let end = self.lexer_offset();
        Ok(self.keyword(KeywordKind::Case, start, end, parts))
    }

    /// Parse one `[(]pattern) body [;;|;&|;;&]` clause.
    fn parse_case_clause(&mut self) -> Result<AstNode, ParseError> {
        let start = self.peek_offset()?;
        let mut inner = Vec::new();
        if matches!(self.peek(0)?, Token::Op(OpKind::LParen, _)) {
            let span = self.peek(0)?.span().unwrap_or(Span { start, end: start });
            self.next()?;
            inner.push(AstNode::ReservedWord {
                span,
                word: "(".to_string(),
            });
        }
        // Pattern alternatives: words joined by `|` reserved words.
        let mut pats = Vec::new();
        loop {
            let tok = self.peek(0)?.clone();
            match tok {
                Token::Word(w) => {
                    let node = AstNode::Word(self.build_word(&w)?);
                    self.next()?;
                    pats.push(node);
                }
                Token::Op(OpKind::Pipe, span) => {
                    self.next()?;
                    inner.push(AstNode::ReservedWord {
                        span,
                        word: "|".to_string(),
                    });
                }
                _ => break,
            }
            if !matches!(self.peek(0)?, Token::Op(OpKind::Pipe, _)) {
                // peek again after a word handled above
            }
            match self.peek(0)? {
                Token::Op(OpKind::Pipe, _) => continue,
                _ => break,
            }
        }
        if pats.is_empty() {
            return Err(ParseError::Unexpected {
                found: Self::token_desc(self.peek(0)?),
                offset: self.peek_offset()?,
            });
        }
        // Splice: leading `(` reserved word, then pattern node, mirroring
        // bashlex pattern nodes holding the alternative words.
        let pat_span = Span::covering(pats.iter().map(|p| p.span())).unwrap_or(Span {
            start,
            end: start,
        });
        inner.push(AstNode::Keyword {
            kind: KeywordKind::Pattern,
            span: pat_span,
            parts: pats,
        });
        match self.peek(0)?.clone() {
            Token::Op(OpKind::RParen, span) => {
                self.next()?;
                inner.push(AstNode::ReservedWord {
                    span,
                    word: ")".to_string(),
                });
            }
            other => {
                return Err(ParseError::Unexpected {
                    found: Self::token_desc(&other),
                    offset: self.peek_offset()?,
                })
            }
        }
        // Body up to the terminator or `esac`.
        if !matches!(self.peek(0)?, Token::Op(OpKind::SemiSemi, _))
            && !matches!(self.peek(0)?, Token::Op(OpKind::SemiAnd, _))
            && !matches!(self.peek(0)?, Token::Op(OpKind::SemiSemiAnd, _))
            && !(matches!(self.peek(0)?, Token::Word(w) if w.cooked == "esac"))
        {
            self.skip_newlines()?;
            if !matches!(self.peek(0)?, Token::Op(OpKind::SemiSemi, _))
                && !matches!(self.peek(0)?, Token::Op(OpKind::SemiAnd, _))
                && !matches!(self.peek(0)?, Token::Op(OpKind::SemiSemiAnd, _))
                && !(matches!(self.peek(0)?, Token::Word(w) if w.cooked == "esac"))
            {
                inner.push(self.parse_command_sequence(&EndSet::top())?);
            }
        }
        match self.peek(0)?.clone() {
            Token::Op(OpKind::SemiSemi, span)
            | Token::Op(OpKind::SemiAnd, span)
            | Token::Op(OpKind::SemiSemiAnd, span) => {
                let text = self.op_text(&span);
                self.next()?;
                inner.push(AstNode::ReservedWord { span, word: text });
            }
            Token::Word(_) | Token::Newline(_) | Token::Eof => {}
            other => {
                return Err(ParseError::Unexpected {
                    found: Self::token_desc(&other),
                    offset: self.peek_offset()?,
                })
            }
        }
        let end = self.lexer_offset();
        Ok(AstNode::Compound {
            span: Span { start, end },
            list: inner,
            redirects: Vec::new(),
        })
    }

    /// Parse `name() body`, `function name body`, `function name() body`.
    fn parse_function(&mut self) -> Result<AstNode, ParseError> {
        let start = self.peek_offset()?;
        let mut parts = Vec::new();
        if let Token::Word(w) = self.peek(0)?.clone() {
            if w.cooked == "function" {
                parts.push(self.reserved("function")?);
            }
        }
        let name = match self.next()? {
            Token::Word(w) => {
                let node = AstNode::Word(self.build_word(&w)?);
                parts.push(node.clone());
                match node {
                    AstNode::Word(w) => w,
                    _ => unreachable!(),
                }
            }
            other => {
                return Err(ParseError::Unexpected {
                    found: Self::token_desc(&other),
                    offset: self.peek_offset()?,
                })
            }
        };
        if matches!(self.peek(0)?, Token::Op(OpKind::LParen, _)) {
            let span = self.peek(0)?.span().unwrap_or(Span { start, end: start });
            self.next()?;
            parts.push(AstNode::ReservedWord {
                span,
                word: "(".to_string(),
            });
            match self.peek(0)? {
                Token::Op(OpKind::RParen, _) => {
                    let span = self.peek(0)?.span().unwrap_or(Span { start, end: start });
                    self.next()?;
                    parts.push(AstNode::ReservedWord {
                        span,
                        word: ")".to_string(),
                    });
                }
                other => {
                    return Err(ParseError::Unexpected {
                        found: Self::token_desc(other),
                        offset: self.peek_offset()?,
                    })
                }
            }
        }
        self.skip_newlines()?;
        let body = self.parse_command_unit(&EndSet::top())?;
        if !matches!(body, AstNode::Compound { .. }) {
            return Err(ParseError::Unexpected {
                found: format!("{} (function body must be a compound command)", body.kind()),
                offset: self.peek_offset()?,
            });
        }
        parts.push(body.clone());
        // Trailing redirects attach to the body (bashlex parity).
        let mut redirects = Vec::new();
        loop {
            let tok = self.peek(0)?.clone();
            let is_redir = match &tok {
                Token::Op(kind, _) => is_redirect_op(*kind),
                Token::Word(w) => self.peek_is_redirect_input(w)?,
                _ => false,
            };
            if !is_redir {
                break;
            }
            redirects.push(self.parse_redirect()?);
        }
        if !redirects.is_empty() {
            if let AstNode::Compound {
                redirects: body_redirs,
                ..
            } = parts.last_mut().unwrap_or(&mut AstNode::Unimplemented {
                span: Span { start, end: start },
                parts: Vec::new(),
            }) {
                body_redirs.extend(redirects);
            }
        }
        let end = self.lexer_offset();
        Ok(AstNode::Function {
            span: Span { start, end },
            name,
            body: Box::new(body),
            parts,
        })
    }

    /// Parse `( ... )` with an optional trailing redirect list.
    fn parse_subshell(&mut self) -> Result<AstNode, ParseError> {
        let start = self.peek_offset()?;
        let lspan = self.peek(0)?.span().unwrap_or(Span { start, end: start });
        self.next()?;
        let mut list = vec![AstNode::ReservedWord {
            span: lspan,
            word: "(".to_string(),
        }];
        self.skip_newlines()?;
        list.push(self.parse_command_sequence(&EndSet {
            stop_rparen: true,
            ..EndSet::top()
        })?);
        self.skip_newlines()?;
        match self.peek(0)?.clone() {
            Token::Op(OpKind::RParen, span) => {
                self.next()?;
                list.push(AstNode::ReservedWord {
                    span,
                    word: ")".to_string(),
                });
            }
            other => {
                return Err(ParseError::Unexpected {
                    found: Self::token_desc(&other),
                    offset: self.peek_offset()?,
                })
            }
        }
        let mut redirects = Vec::new();
        redirects.extend(self.parse_trailing_redirects()?);
        let end = self.lexer_offset();
        Ok(AstNode::Compound {
            span: Span { start, end },
            list,
            redirects,
        })
    }

    /// Parse `{ ...; }` with an optional trailing redirect list.
    fn parse_group(&mut self) -> Result<AstNode, ParseError> {
        let start = self.peek_offset()?;
        let lspan = self.peek(0)?.span().unwrap_or(Span { start, end: start });
        self.next()?;
        let mut list = vec![AstNode::ReservedWord {
            span: lspan,
            word: "{".to_string(),
        }];
        self.skip_newlines()?;
        list.push(self.parse_command_sequence(&EndSet {
            stop_rbrace: true,
            ..EndSet::top()
        })?);
        self.skip_newlines()?;
        match self.peek(0)?.clone() {
            Token::Op(OpKind::RBrace, span) => {
                self.next()?;
                list.push(AstNode::ReservedWord {
                    span,
                    word: "}".to_string(),
                });
            }
            other => {
                return Err(ParseError::Unexpected {
                    found: Self::token_desc(&other),
                    offset: self.peek_offset()?,
                })
            }
        }
        let mut redirects = Vec::new();
        redirects.extend(self.parse_trailing_redirects()?);
        let end = self.lexer_offset();
        Ok(AstNode::Compound {
            span: Span { start, end },
            list,
            redirects,
        })
    }

    /// Parse redirects following a compound command or function body.
    fn parse_trailing_redirects(&mut self) -> Result<Vec<AstNode>, ParseError> {
        let mut redirects = Vec::new();
        loop {
            let tok = self.peek(0)?.clone();
            let is_redir = match &tok {
                Token::Op(kind, _) => is_redirect_op(*kind),
                Token::Word(w) => self.peek_is_redirect_input(w)?,
                _ => false,
            };
            if !is_redir {
                break;
            }
            redirects.push(self.parse_redirect()?);
        }
        Ok(redirects)
    }

    /// Parse `time` / `coproc` by wrapping the following pipeline in an
    /// Unimplemented node (bashlex raises; we degrade gracefully).
    fn parse_unimplemented_prefix(&mut self) -> Result<AstNode, ParseError> {
        let start = self.peek_offset()?;
        let word = match self.next()? {
            Token::Word(w) => w,
            other => {
                return Err(ParseError::Unexpected {
                    found: Self::token_desc(&other),
                    offset: self.peek_offset()?,
                })
            }
        };
        let mut parts = vec![AstNode::ReservedWord {
            span: word.span,
            word: word.cooked.clone(),
        }];
        if self.at_command_start()? {
            parts.push(self.parse_pipeline(&EndSet::top())?);
        }
        let end = self.lexer_offset();
        Ok(AstNode::Unimplemented {
            span: Span { start, end },
            parts,
        })
    }

    /// Skip a balanced `(...)` region into an Unimplemented node
    /// (arithmetic-for).
    fn parse_unimplemented_balanced(
        &mut self,
        keyword: &AstNode,
        what: &str,
    ) -> Result<AstNode, ParseError> {
        let start = keyword.span().start;
        let mut parts = vec![keyword.clone()];
        let mut depth = 0i32;
        loop {
            match self.next()? {
                Token::Op(OpKind::LParen, span) => {
                    depth += 1;
                    parts.push(AstNode::ReservedWord {
                        span,
                        word: "(".to_string(),
                    });
                }
                Token::Op(OpKind::RParen, span) => {
                    parts.push(AstNode::ReservedWord {
                        span,
                        word: ")".to_string(),
                    });
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                Token::Eof => {
                    return Err(ParseError::Unimplemented {
                        what: what.to_string(),
                        offset: start,
                    })
                }
                Token::Word(w) => parts.push(AstNode::Word(self.build_word(&w)?)),
                Token::Newline(_) => {}
                Token::Op(kind, span) => parts.push(AstNode::ReservedWord {
                    span,
                    word: kind.text().to_string(),
                }),
            }
        }
        let end = self.lexer_offset();
        Ok(AstNode::Unimplemented {
            span: Span { start, end },
            parts,
        })
    }

    /// Consume one of the given reserved words.
    fn reserved_any(&mut self, words: &[&str]) -> Result<AstNode, ParseError> {
        match self.peek(0)?.clone() {
            Token::Word(w) if words.contains(&w.cooked.as_str()) => {
                self.next()?;
                Ok(AstNode::ReservedWord {
                    span: w.span,
                    word: w.cooked.clone(),
                })
            }
            other => Err(ParseError::Unexpected {
                found: Self::token_desc(&other),
                offset: self.peek_offset()?,
            }),
        }
    }

    /// Consume the next word token.
    fn expect_word(&mut self) -> Result<RawWord, ParseError> {
        match self.next()? {
            Token::Word(w) => Ok(w),
            other => Err(ParseError::Unexpected {
                found: Self::token_desc(&other),
                offset: self.peek_offset()?,
            }),
        }
    }

    /// Build a Keyword node spanning the parts.
    fn keyword(&self, kind: KeywordKind, start: usize, end: usize, parts: Vec<AstNode>) -> AstNode {
        let span = Span::covering(parts.iter().map(|p| p.span())).unwrap_or(Span { start, end });
        AstNode::Keyword { kind, span, parts }
    }
}

/// True for redirect operators.
fn is_redirect_op(kind: OpKind) -> bool {
    matches!(
        kind,
        OpKind::Less
            | OpKind::Greater
            | OpKind::GreaterGreater
            | OpKind::LessLess
            | OpKind::LessLessMinus
            | OpKind::LessLessLess
            | OpKind::LessAnd
            | OpKind::GreaterAnd
            | OpKind::AndGreater
            | OpKind::AndGreaterGreater
            | OpKind::LessGreater
            | OpKind::GreaterBar
    )
}

/// True for non-empty all-digit words (fd numbers).
fn is_all_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit())
}

/// True for `{name}`-shaped words (fd redirections).
fn is_redir_var(s: &str) -> bool {
    s.len() > 2 && s.starts_with('{') && s.ends_with('}')
}

/// True when a command-head word is an assignment (`NAME=value`,
/// `NAME+=value`) with no quoting before the `=`.
fn is_assignment(word: &RawWord, input: &[u8]) -> bool {
    let eq = match word.cooked.find('=') {
        Some(i) if i > 0 => i,
        _ => return false,
    };
    let mut name = &word.cooked[..eq];
    if let Some(stripped) = name.strip_suffix('+') {
        name = stripped;
    }
    if name.is_empty()
        || !name
            .bytes()
            .enumerate()
            .all(|(i, c)| c == b'_' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit()))
    {
        return false;
    }
    // The raw prefix must match byte-for-byte (no quotes or escapes).
    let raw = &input[word.span.start..word.span.end.min(input.len())];
    match raw.iter().position(|&c| c == b'=') {
        Some(i) => raw.get(..i) == Some(name.as_bytes()),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn single(input: &str) -> AstNode {
        parsesingle(input, true, None).expect("parse failed")
    }

    fn words_of(node: &AstNode) -> Vec<&WordNode> {
        match node {
            AstNode::Command { parts, .. } => parts
                .iter()
                .filter_map(|p| match p {
                    AstNode::Word(w) | AstNode::Assignment(w) => Some(w),
                    _ => None,
                })
                .collect(),
            _ => panic!("expected command, got {}", node.kind()),
        }
    }

    #[test]
    fn simple_command_spans() {
        let node = single("tar -xvf archive.tar");
        let words = words_of(&node);
        assert_eq!(words.len(), 3);
        assert_eq!(words[0].text, "tar");
        assert_eq!(words[0].span, Span { start: 0, end: 3 });
        assert_eq!(words[1].text, "-xvf");
        assert_eq!(words[2].text, "archive.tar");
        assert_eq!(node.span(), Span { start: 0, end: 20 });
    }

    #[test]
    fn quoted_word_keeps_raw_span() {
        let node = single("echo \"a b\"");
        let words = words_of(&node);
        assert_eq!(words[1].text, "a b");
        assert_eq!(words[1].span.len(), 5);
        assert!(words[1].was_quoted());
    }

    #[test]
    fn assignments_only_at_command_start() {
        let node = single("A=1 B=2 cmd");
        match &node {
            AstNode::Command { parts, .. } => {
                assert_eq!(parts.len(), 3);
                assert_eq!(parts[0].kind(), "assignment");
                assert_eq!(parts[1].kind(), "assignment");
                assert_eq!(parts[2].kind(), "word");
            }
            _ => panic!("expected command"),
        }
        // After the program name, `A=1` is a plain argument.
        let node = single("echo A=1");
        match &node {
            AstNode::Command { parts, .. } => {
                assert_eq!(parts[1].kind(), "word");
            }
            _ => panic!("expected command"),
        }
        // Quoted names are not assignments.
        let node = single("'A=1' cmd");
        match &node {
            AstNode::Command { parts, .. } => {
                assert_eq!(parts[0].kind(), "word");
            }
            _ => panic!("expected command"),
        }
    }

    #[test]
    fn pipelines_and_negation() {
        let node = single("a | b | c");
        match &node {
            AstNode::Pipeline { parts, .. } => {
                assert_eq!(parts.len(), 5);
                assert_eq!(parts[1].kind(), "pipe");
            }
            _ => panic!("expected pipeline, got {}", node.kind()),
        }
        let node = single("! a");
        match &node {
            AstNode::Pipeline { parts, .. } => {
                assert_eq!(parts[0].kind(), "reservedword");
            }
            _ => panic!("expected pipeline"),
        }
        let node = single("a |& b");
        match &node {
            AstNode::Pipeline { parts, .. } => {
                assert!(matches!(parts[1], AstNode::Pipe { .. }));
            }
            _ => panic!("expected pipeline"),
        }
    }

    #[test]
    fn lists_and_trailing_separators() {
        let node = single("a; b");
        match &node {
            AstNode::List { parts, .. } => {
                assert_eq!(parts.len(), 3);
                assert_eq!(parts[1].kind(), "operator");
            }
            _ => panic!("expected list"),
        }
        let node = single("a && b || c");
        match &node {
            AstNode::List { parts, .. } => assert_eq!(parts.len(), 5),
            _ => panic!("expected list"),
        }
        // Trailing separators stay in the list.
        let node = single("a;");
        match &node {
            AstNode::List { parts, .. } => assert_eq!(parts.len(), 2),
            _ => panic!("expected list"),
        }
        // `&&` demands a right-hand side.
        assert!(parsesingle("a &&", true, None).is_err());
    }

    #[test]
    fn parsesingle_stops_at_newline() {
        let node = single("a\nb");
        match &node {
            AstNode::Command { parts, .. } => {
                assert_eq!(parts.len(), 1);
            }
            _ => panic!("expected single command, got {}", node.kind()),
        }
        let parts = parse("a\nb", true, None).unwrap();
        assert_eq!(parts.len(), 2);
    }

    #[test]
    fn redirects() {
        let node = single("echo hi >out.txt");
        match &node {
            AstNode::Command { parts, .. } => {
                assert_eq!(parts.len(), 3);
                match &parts[2] {
                    AstNode::Redirect { op, output, .. } => {
                        assert_eq!(op, ">");
                        assert!(matches!(output, RedirectTarget::Word(_)));
                    }
                    _ => panic!("expected redirect"),
                }
            }
            _ => panic!("expected command"),
        }
        // fd duplication: both sides numeric.
        let node = single("cmd 2>&1");
        match &node {
            AstNode::Command { parts, .. } => match &parts[1] {
                AstNode::Redirect { input, op, output, .. } => {
                    assert_eq!(op, ">&");
                    assert_eq!(*input, Some(RedirectSource::Fd(2)));
                    assert_eq!(*output, RedirectTarget::Fd(1));
                }
                _ => panic!("expected redirect"),
            },
            _ => panic!("expected command"),
        }
        // Closing dash.
        let node = single("cmd >&-");
        match &node {
            AstNode::Command { parts, .. } => match &parts[1] {
                AstNode::Redirect { output, .. } => {
                    assert_eq!(*output, RedirectTarget::Dash);
                }
                _ => panic!("expected redirect"),
            },
            _ => panic!("expected command"),
        }
    }

    #[test]
    fn heredoc_gathering() {
        let node = single("cat <<EOF\nhello\nEOF\n");
        match &node {
            AstNode::Command { parts, .. } => match &parts[1] {
                AstNode::Redirect { op, heredoc, .. } => {
                    assert_eq!(op, "<<");
                    let h = heredoc.as_ref().expect("heredoc body");
                    match h.as_ref() {
                        AstNode::Heredoc { value, .. } => assert_eq!(value, "hello\n"),
                        _ => panic!("expected heredoc"),
                    }
                }
                _ => panic!("expected redirect"),
            },
            _ => panic!("expected command"),
        }
        // The body does not leak into later commands.
        let parts = parse("cat <<EOF\nhello\nEOF\necho done", true, None).unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[1].kind(), "command");
    }

    #[test]
    fn heredoc_missing_non_strict() {
        let node = parsesingle("cat <<EOF", false, None).expect("non-strict parses");
        match &node {
            AstNode::Command { parts, .. } => match &parts[1] {
                AstNode::Redirect { heredoc, .. } => assert!(heredoc.is_none()),
                _ => panic!("expected redirect"),
            },
            _ => panic!("expected command"),
        }
        assert!(parsesingle("cat <<EOF", true, None).is_err());
    }

    #[test]
    fn if_command_shape() {
        let node = single("if a; then b; else c; fi");
        match &node {
            AstNode::Keyword { kind, parts, .. } => {
                assert_eq!(*kind, KeywordKind::If);
                let kinds: Vec<&str> = parts.iter().map(|p| p.kind()).collect();
                assert_eq!(
                    kinds,
                    vec![
                        "reservedword",
                        "command",
                        "reservedword",
                        "command",
                        "reservedword",
                        "command",
                        "reservedword"
                    ]
                );
            }
            _ => panic!("expected if, got {}", node.kind()),
        }
    }

    #[test]
    fn for_command_shape() {
        let node = single("for x in a b; do echo; done");
        match &node {
            AstNode::Keyword { kind, parts, .. } => {
                assert_eq!(*kind, KeywordKind::For);
                // for-rw, x, in-rw, a, b, ;-reservedword, do-rw, body, done-rw
                assert_eq!(parts[0].kind(), "reservedword");
                assert_eq!(parts[2].kind(), "reservedword");
                assert_eq!(parts[5].kind(), "reservedword");
            }
            _ => panic!("expected for, got {}", node.kind()),
        }
    }

    #[test]
    fn while_until_case_function() {
        let node = single("while a; do b; done");
        assert_eq!(node.kind(), "while");
        let node = single("until a; do b; done");
        assert_eq!(node.kind(), "until");
        let node = single("case x in a) b;; c) d;; esac");
        assert_eq!(node.kind(), "case");
        let node = single("f() { echo hi; }");
        match &node {
            AstNode::Function { name, body, .. } => {
                assert_eq!(name.text, "f");
                assert_eq!(body.kind(), "compound");
            }
            _ => panic!("expected function, got {}", node.kind()),
        }
        let node = single("(a; b)");
        assert_eq!(node.kind(), "compound");
        let node = single("{ a; b; }");
        assert_eq!(node.kind(), "compound");
    }

    #[test]
    fn substitution_depth_limit() {
        // Limit 1 (matcher usage): top-level children exist...
        let node = parsesingle("echo $(foo $bar)", true, Some(1)).unwrap();
        let words = words_of(&node);
        assert_eq!(words[1].parts.len(), 1);
        assert_eq!(words[1].parts[0].kind(), "commandsubstitution");
        // ...but substitutions nested inside are filtered out.
        let node = parsesingle("echo $(foo $(bar))", true, Some(1)).unwrap();
        let words = words_of(&node);
        let inner = match &words[1].parts[0] {
            AstNode::CommandSubstitution { command, .. } => command,
            _ => panic!("expected substitution"),
        };
        let inner_words = match inner.as_ref() {
            AstNode::Command { parts, .. } => parts
                .iter()
                .filter_map(|p| match p {
                    AstNode::Word(w) => Some(w),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            _ => panic!("expected inner command"),
        };
        assert!(inner_words[1].parts.is_empty());
        // Unbounded: the nested child survives.
        let node = parsesingle("echo $(foo $(bar))", true, None).unwrap();
        let words = words_of(&node);
        assert_eq!(words[1].parts.len(), 1);
    }

    #[test]
    fn substitution_positions_are_absolute() {
        let input = "echo $(foo)";
        let node = parsesingle(input, true, None).unwrap();
        let words = words_of(&node);
        match &words[1].parts[0] {
            AstNode::CommandSubstitution { span, command } => {
                assert_eq!(&input[span.start..span.end], "$(foo)");
                assert_eq!(&input[command.span().start..command.span().end], "foo");
            }
            _ => panic!("expected substitution"),
        }
    }

    #[test]
    fn tilde_and_parameter_children() {
        let node = single("echo ~/bin $HOME");
        let words = words_of(&node);
        assert_eq!(words[1].parts[0].kind(), "tilde");
        assert_eq!(words[2].parts[0].kind(), "parameter");
        assert_eq!(&"echo ~/bin $HOME"[words[1].parts[0].span().start..words[1].parts[0].span().end], "~");
    }

    #[test]
    fn graceful_degradations() {
        // `[[...]]` parses as an (unknown) simple command.
        let node = single("[[ -f x ]] && echo");
        assert_eq!(node.kind(), "list");
        // `time` wraps the pipeline instead of raising.
        let node = single("time foo | bar");
        assert_eq!(node.kind(), "unimplemented");
        // `select` shares the for shape.
        let node = single("select x in a b; do echo; done");
        assert_eq!(node.kind(), "for");
        // Stray closers are errors, empty input is an error.
        assert!(parsesingle(")", true, None).is_err());
        assert!(parsesingle("", true, None).is_err());
        assert!(parsesingle("echo $(foo", true, None).is_err());
    }
}
