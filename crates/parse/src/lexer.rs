//! Shell lexer for explainshell WASM.
//!
//! Hand-written scanner producing bashlex-compatible tokens. Words are
//! scanned with quote removal and expansion-span recording in one pass
//! (mirroring bashlex `_readtokenword` + `subst` expansion): the cooked
//! text keeps expansions verbatim while `span` covers the raw source.
//!
//! Simplifications over bashlex (documented, tested):
//! - `$'...'` / `$"..."` spans are kept verbatim (no ANSI-C unescaping).
//! - Command substitutions are heredoc-aware but not arithmetic-aware
//!   (`$((...))` scans as a substitution span).
//! - Positions are byte offsets; non-ASCII input keeps correct spans but
//!   callers slicing by chars must account for UTF-8 widths.

use crate::ast::Span;

/// Lexer failure with the byte offset where scanning stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexError {
    /// Human-readable cause
    pub message: String,
    /// Byte offset of the failure
    pub offset: usize,
}

impl LexError {
    fn new(message: impl Into<String>, offset: usize) -> LexError {
        LexError {
            message: message.into(),
            offset,
        }
    }
}

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at offset {}", self.message, self.offset)
    }
}

impl std::error::Error for LexError {}

/// Raw expansion recorded while scanning a word.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawExpansion {
    /// `$(...)` or backquotes; `inner` excludes the delimiters
    CmdSubst {
        /// Whole span including delimiters
        span: Span,
        /// Span of the inner command text
        inner: Span,
    },
    /// `<(...)` / `>(...)`; `inner` excludes the delimiters
    ProcSubst {
        /// Whole span including delimiters
        span: Span,
        /// Span of the inner command text
        inner: Span,
    },
    /// `$name`, `${...}`, `$1`, `$*`, `$@`, `$?`, `$$`, ...
    Param {
        /// Whole span
        span: Span,
    },
    /// `~`, `~/...`, `~user/...` (the `~user` prefix span)
    Tilde {
        /// Span of the tilde prefix
        span: Span,
    },
}

/// A scanned word: cooked text plus expansion spans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawWord {
    /// Raw source span (includes quotes)
    pub span: Span,
    /// Quote-removed text, expansions kept verbatim
    pub cooked: String,
    /// A quote or escape was processed
    pub quoted: bool,
    /// A `$` expansion was seen
    pub has_dollar: bool,
    /// Expansions in source order
    pub expansions: Vec<RawExpansion>,
}

/// Shell operators (bashlex token types that reach the grammar).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpKind {
    /// `;`
    Semi,
    /// `&`
    Amp,
    /// `&&`
    AndAnd,
    /// `||`
    OrOr,
    /// `|`
    Pipe,
    /// `|&`
    PipeAmp,
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `{`
    LBrace,
    /// `}`
    RBrace,
    /// `!`
    Bang,
    /// `<`
    Less,
    /// `>`
    Greater,
    /// `>>`
    GreaterGreater,
    /// `<<`
    LessLess,
    /// `<<-`
    LessLessMinus,
    /// `<<<`
    LessLessLess,
    /// `<&`
    LessAnd,
    /// `>&`
    GreaterAnd,
    /// `&>`
    AndGreater,
    /// `&>>`
    AndGreaterGreater,
    /// `<>`
    LessGreater,
    /// `>|`
    GreaterBar,
    /// `;;`
    SemiSemi,
    /// `;&`
    SemiAnd,
    /// `;;&`
    SemiSemiAnd,
    /// `-` (only after `<&` / `>&`)
    Dash,
}

impl OpKind {
    /// Operator text (bashlex token values).
    pub fn text(&self) -> &'static str {
        match self {
            OpKind::Semi => ";",
            OpKind::Amp => "&",
            OpKind::AndAnd => "&&",
            OpKind::OrOr => "||",
            OpKind::Pipe => "|",
            OpKind::PipeAmp => "|&",
            OpKind::LParen => "(",
            OpKind::RParen => ")",
            OpKind::LBrace => "{",
            OpKind::RBrace => "}",
            OpKind::Bang => "!",
            OpKind::Less => "<",
            OpKind::Greater => ">",
            OpKind::GreaterGreater => ">>",
            OpKind::LessLess => "<<",
            OpKind::LessLessMinus => "<<-",
            OpKind::LessLessLess => "<<<",
            OpKind::LessAnd => "<&",
            OpKind::GreaterAnd => ">&",
            OpKind::AndGreater => "&>",
            OpKind::AndGreaterGreater => "&>>",
            OpKind::LessGreater => "<>",
            OpKind::GreaterBar => ">|",
            OpKind::SemiSemi => ";;",
            OpKind::SemiAnd => ";&",
            OpKind::SemiSemiAnd => ";;&",
            OpKind::Dash => "-",
        }
    }
}

/// Lexer token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    /// Scanned word
    Word(RawWord),
    /// Newline (comments already stripped)
    Newline(Span),
    /// Operator
    Op(OpKind, Span),
    /// End of input
    Eof,
}

impl Token {
    /// Source span of the token, if it has one.
    pub fn span(&self) -> Option<Span> {
        match self {
            Token::Word(w) => Some(w.span),
            Token::Newline(s) => Some(*s),
            Token::Op(_, s) => Some(*s),
            Token::Eof => None,
        }
    }
}

/// Byte-oriented shell scanner.
pub struct Lexer<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> Lexer<'a> {
    /// Create a scanner over the input text.
    pub fn new(input: &'a str) -> Lexer<'a> {
        Lexer {
            input: input.as_bytes(),
            pos: 0,
        }
    }

    /// Current byte offset.
    pub fn offset(&self) -> usize {
        self.pos
    }

    /// Jump forward to at least `off` (heredoc body skipping). Never moves
    /// backwards.
    pub fn skip_to(&mut self, off: usize) {
        self.pos = self.pos.max(off);
    }

    /// Peek the current byte without consuming.
    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    /// Peek the byte after the current one.
    fn peek2(&self) -> Option<u8> {
        self.input.get(self.pos + 1).copied()
    }

    /// Consume and return the current byte.
    fn next(&mut self) -> Option<u8> {
        let c = self.peek()?;
        self.pos += 1;
        Some(c)
    }

    /// True at end of input.
    fn at_end(&self) -> bool {
        self.pos >= self.input.len()
    }

    /// Skip blanks, line continuations, and return when a real char is found.
    fn skip_blanks(&mut self) {
        loop {
            match self.peek() {
                Some(b' ') | Some(b'\t') => {
                    self.pos += 1;
                }
                Some(b'\\') if self.peek2() == Some(b'\n') => {
                    // Line continuation outside words vanishes entirely.
                    self.pos += 2;
                }
                _ => break,
            }
        }
    }

    /// Read the next token.
    pub fn next_token(&mut self) -> Result<Token, LexError> {
        self.skip_blanks();
        let start = self.pos;
        match self.peek() {
            None => Ok(Token::Eof),
            Some(b'\n') => {
                self.pos += 1;
                Ok(Token::Newline(Span {
                    start,
                    end: self.pos,
                }))
            }
            Some(b'#') => {
                // Comment: only valid at a token boundary, which is exactly
                // where next_token is called.
                while matches!(self.peek(), Some(c) if c != b'\n') {
                    self.pos += 1;
                }
                self.next_token()
            }
            Some(c) if is_operator_start(c) => {
                // `<(` / `>(` open a process substitution word, not a
                // redirect (mirrors bashlex falling through to
                // _readtokenword when peek is `(` after `<` / `>`).
                if (c == b'<' || c == b'>') && self.peek2() == Some(b'(') {
                    let word = self.read_word()?;
                    Ok(Token::Word(word))
                } else {
                    self.read_operator()
                }
            }
            _ => {
                let word = self.read_word()?;
                Ok(Token::Word(word))
            }
        }
    }

    /// Read an operator with longest-match (mirrors bashlex _readtoken).
    fn read_operator(&mut self) -> Result<Token, LexError> {
        let start = self.pos;
        let c = self.next().unwrap_or(0);
        let p = self.peek();
        let kind = match c {
            b';' => match p {
                Some(b';') => {
                    self.pos += 1;
                    match self.peek() {
                        Some(b'&') => {
                            self.pos += 1;
                            OpKind::SemiSemiAnd
                        }
                        _ => OpKind::SemiSemi,
                    }
                }
                Some(b'&') => {
                    self.pos += 1;
                    OpKind::SemiAnd
                }
                _ => OpKind::Semi,
            },
            b'&' => match p {
                Some(b'&') => {
                    self.pos += 1;
                    OpKind::AndAnd
                }
                Some(b'>') => {
                    self.pos += 1;
                    match self.peek() {
                        Some(b'>') => {
                            self.pos += 1;
                            OpKind::AndGreaterGreater
                        }
                        _ => OpKind::AndGreater,
                    }
                }
                _ => OpKind::Amp,
            },
            b'|' => match p {
                Some(b'|') => {
                    self.pos += 1;
                    OpKind::OrOr
                }
                Some(b'&') => {
                    self.pos += 1;
                    OpKind::PipeAmp
                }
                _ => OpKind::Pipe,
            },
            b'<' => match p {
                Some(b'<') => {
                    self.pos += 1;
                    match self.peek() {
                        Some(b'-') => {
                            self.pos += 1;
                            OpKind::LessLessMinus
                        }
                        Some(b'<') => {
                            self.pos += 1;
                            OpKind::LessLessLess
                        }
                        _ => OpKind::LessLess,
                    }
                }
                Some(b'&') => {
                    self.pos += 1;
                    OpKind::LessAnd
                }
                Some(b'>') => {
                    self.pos += 1;
                    OpKind::LessGreater
                }
                _ => OpKind::Less,
            },
            b'>' => match p {
                Some(b'>') => {
                    self.pos += 1;
                    OpKind::GreaterGreater
                }
                Some(b'&') => {
                    self.pos += 1;
                    OpKind::GreaterAnd
                }
                Some(b'|') => {
                    self.pos += 1;
                    OpKind::GreaterBar
                }
                _ => OpKind::Greater,
            },
            b'(' => OpKind::LParen,
            b')' => OpKind::RParen,
            b'{' => OpKind::LBrace,
            b'}' => OpKind::RBrace,
            b'!' => OpKind::Bang,
            _ => {
                return Err(LexError::new(
                    format!("unexpected operator byte {c}"),
                    start,
                ))
            }
        };
        Ok(Token::Op(
            kind,
            Span {
                start,
                end: self.pos,
            },
        ))
    }

    /// Read one word with quote removal and expansion recording.
    pub fn read_word(&mut self) -> Result<RawWord, LexError> {
        let start = self.pos;
        let mut cooked = String::new();
        let mut quoted = false;
        let mut has_dollar = false;
        let mut expansions: Vec<RawExpansion> = Vec::new();
        // Tilde expands at word start or right after `=` / `:`.
        let mut tilde_ok = true;

        loop {
            // `<(` / `>(` at word start open process substitution rather
            // than terminating the word.
            let at_procsubst = cooked.is_empty()
                && matches!(self.peek(), Some(b'<') | Some(b'>'))
                && self.peek2() == Some(b'(');
            let c = match self.peek() {
                None => break,
                Some(b'\n') | Some(b' ') | Some(b'\t') => break,
                Some(c) if is_word_break(c) && !at_procsubst => break,
                Some(c) => c,
            };
            match c {
                b'\'' => {
                    quoted = true;
                    tilde_ok = false;
                    self.pos += 1;
                    loop {
                        match self.next() {
                            None => {
                                return Err(LexError::new("unterminated single quote", start));
                            }
                            Some(b'\'') => break,
                            Some(ch) => cooked.push(ch as char),
                        }
                    }
                }
                b'"' => {
                    quoted = true;
                    tilde_ok = false;
                    self.pos += 1;
                    loop {
                        match self.next() {
                            None => {
                                return Err(LexError::new("unterminated double quote", start));
                            }
                            Some(b'"') => break,
                            Some(b'\\') => match self.peek() {
                                Some(n @ (b'$' | b'`' | b'"' | b'\\' | b'\n')) => {
                                    self.pos += 1;
                                    // Line continuation inside quotes vanishes.
                                    if n != b'\n' {
                                        cooked.push(n as char);
                                    }
                                }
                                _ => cooked.push('\\'),
                            },
                            Some(b'$') => {
                                self.scan_dollar(
                                    &mut cooked,
                                    &mut expansions,
                                    &mut has_dollar,
                                    &mut quoted,
                                )?;
                            }
                            Some(b'`') => {
                                self.scan_backquote(&mut cooked, &mut expansions)?;
                                has_dollar = true;
                            }
                            Some(ch) => cooked.push(ch as char),
                        }
                    }
                }
                b'\\' => {
                    quoted = true;
                    tilde_ok = false;
                    self.pos += 1;
                    match self.next() {
                        None => cooked.push('\\'),
                        Some(b'\n') => {
                            // Line continuation vanishes.
                        }
                        Some(ch) => cooked.push(ch as char),
                    }
                }
                b'$' => {
                    tilde_ok = false;
                    self.scan_dollar(&mut cooked, &mut expansions, &mut has_dollar, &mut quoted)?;
                }
                b'`' => {
                    tilde_ok = false;
                    self.scan_backquote(&mut cooked, &mut expansions)?;
                    has_dollar = true;
                }
                b'~' if tilde_ok => {
                    tilde_ok = false;
                    self.scan_tilde(&mut cooked, &mut expansions)?;
                }
                b'<' | b'>' if cooked.is_empty() && self.peek2() == Some(b'(') => {
                    // Process substitution at word start: `<(...)` / `>(...)`.
                    let start = self.pos;
                    self.pos += 2;
                    let inner_start = self.pos;
                    let close = self.scan_balanced(b'(', b')', start)?;
                    expansions.push(RawExpansion::ProcSubst {
                        span: Span {
                            start,
                            end: close,
                        },
                        inner: Span {
                            start: inner_start,
                            end: close - 1,
                        },
                    });
                    cooked.push_str(&String::from_utf8_lossy(&self.input[start..close]));
                    has_dollar = true;
                    tilde_ok = false;
                }
                _ => {
                    let ch = c as char;
                    cooked.push(ch);
                    tilde_ok = c == b'=' || c == b':';
                    self.pos += 1;
                }
            }
        }

        assert!(
            self.pos > start,
            "lexer produced an empty word (infinite-loop guard)"
        );
        Ok(RawWord {
            span: Span {
                start,
                end: self.pos,
            },
            cooked,
            quoted,
            has_dollar,
            expansions,
        })
    }

    /// Scan a `$...` expansion. The leading `$` is current.
    fn scan_dollar(
        &mut self,
        cooked: &mut String,
        expansions: &mut Vec<RawExpansion>,
        has_dollar: &mut bool,
        quoted: &mut bool,
    ) -> Result<(), LexError> {
        let dollar = self.pos;
        debug_assert_eq!(self.peek(), Some(b'$'));
        self.pos += 1;
        match self.peek() {
            Some(b'(') => {
                self.pos += 1;
                let inner_start = self.pos;
                let close = self.scan_balanced(b'(', b')', dollar)?;
                expansions.push(RawExpansion::CmdSubst {
                    span: Span {
                        start: dollar,
                        end: close,
                    },
                    inner: Span {
                        start: inner_start,
                        end: close - 1,
                    },
                });
                cooked.push_str(&String::from_utf8_lossy(&self.input[dollar..close]));
                *has_dollar = true;
            }
            Some(b'{') => {
                self.pos += 1;
                let close = self.scan_balanced(b'{', b'}', dollar)?;
                expansions.push(RawExpansion::Param {
                    span: Span {
                        start: dollar,
                        end: close,
                    },
                });
                cooked.push_str(&String::from_utf8_lossy(&self.input[dollar..close]));
                *has_dollar = true;
            }
            Some(b'[') => {
                // Old `$[...]` arithmetic: keep the span, no expansion node
                // (bashlex has no arithmetic support either).
                self.pos += 1;
                let close = self.scan_balanced(b'[', b']', dollar)?;
                cooked.push_str(&String::from_utf8_lossy(&self.input[dollar..close]));
                *has_dollar = true;
            }
            Some(b'\'') | Some(b'"') => {
                // `$'...'` / `$"..."`: keep verbatim (no ANSI-C unescaping).
                let quote = self.peek().unwrap_or(0);
                *quoted = true;
                let end = self.scan_quoted(quote, dollar)?;
                cooked.push_str(&String::from_utf8_lossy(&self.input[dollar..end]));
                *has_dollar = true;
            }
            Some(b'$') | Some(b'?') | Some(b'#') | Some(b'*') | Some(b'@') | Some(b'!')
            | Some(b'-') => {
                self.pos += 1;
                expansions.push(RawExpansion::Param {
                    span: Span {
                        start: dollar,
                        end: self.pos,
                    },
                });
                cooked.push_str(&String::from_utf8_lossy(&self.input[dollar..self.pos]));
                *has_dollar = true;
            }
            Some(c) if c.is_ascii_digit() => {
                self.pos += 1;
                expansions.push(RawExpansion::Param {
                    span: Span {
                        start: dollar,
                        end: self.pos,
                    },
                });
                cooked.push_str(&String::from_utf8_lossy(&self.input[dollar..self.pos]));
                *has_dollar = true;
            }
            Some(c) if is_name_start(c) => {
                self.pos += 1;
                while matches!(self.peek(), Some(c) if is_name_char(c)) {
                    self.pos += 1;
                }
                expansions.push(RawExpansion::Param {
                    span: Span {
                        start: dollar,
                        end: self.pos,
                    },
                });
                cooked.push_str(&String::from_utf8_lossy(&self.input[dollar..self.pos]));
                *has_dollar = true;
            }
            _ => {
                // Lone `$` before a break char: literal.
                cooked.push('$');
            }
        }
        Ok(())
    }

    /// Scan a backquote substitution. The leading backquote is current.
    fn scan_backquote(
        &mut self,
        cooked: &mut String,
        expansions: &mut Vec<RawExpansion>,
    ) -> Result<(), LexError> {
        let start = self.pos;
        debug_assert_eq!(self.peek(), Some(b'`'));
        self.pos += 1;
        let inner_start = self.pos;
        let mut depth = 0usize;
        loop {
            match self.next() {
                None => return Err(LexError::new("unterminated backquote", start)),
                Some(b'\\') => {
                    // Keep the escape pair for the recursive parse to handle.
                    if self.peek().is_some() {
                        self.pos += 1;
                    }
                }
                Some(b'$') if self.peek() == Some(b'(') => {
                    depth += 1;
                    self.pos += 1;
                }
                Some(b'(') if depth > 0 => {
                    depth += 1;
                }
                Some(b')') if depth > 0 => {
                    depth -= 1;
                }
                Some(b'`') if depth == 0 => break,
                Some(_) => {}
            }
        }
        let close = self.pos;
        expansions.push(RawExpansion::CmdSubst {
            span: Span { start, end: close },
            inner: Span {
                start: inner_start,
                end: close - 1,
            },
        });
        cooked.push_str(&String::from_utf8_lossy(&self.input[start..close]));
        Ok(())
    }

    /// Scan a `~` / `~user` prefix. The `~` is current.
    fn scan_tilde(
        &mut self,
        cooked: &mut String,
        expansions: &mut Vec<RawExpansion>,
    ) -> Result<(), LexError> {
        let start = self.pos;
        self.pos += 1;
        while matches!(self.peek(), Some(c) if is_name_char(c) || c == b'.' || c == b'-') {
            self.pos += 1;
        }
        expansions.push(RawExpansion::Tilde {
            span: Span {
                start,
                end: self.pos,
            },
        });
        cooked.push_str(&String::from_utf8_lossy(&self.input[start..self.pos]));
        Ok(())
    }

    /// Scan a single-quoted-style span starting at the quote; returns the
    /// offset just past the closing quote.
    fn scan_quoted(&mut self, quote: u8, start: usize) -> Result<usize, LexError> {
        debug_assert_eq!(self.peek(), Some(quote));
        self.pos += 1;
        loop {
            match self.next() {
                None => return Err(LexError::new("unterminated quote", start)),
                Some(b'\\') if quote == b'\'' => {
                    // Inside $'...', backslash escapes the next char.
                    if self.peek().is_some() {
                        self.pos += 1;
                    }
                }
                Some(c) if c == quote => return Ok(self.pos),
                Some(_) => {}
            }
        }
    }

    /// Scan a balanced `open...close` region starting just past `open`.
    /// Handles nesting, quotes, escapes, `#` comments, and heredocs.
    /// Returns the offset just past the closing delimiter.
    fn scan_balanced(&mut self, open: u8, close: u8, start: usize) -> Result<usize, LexError> {
        let mut depth = 1usize;
        while depth > 0 {
            match self.next() {
                None => {
                    return Err(LexError::new(
                        format!("unterminated {}", open as char),
                        start,
                    ));
                }
                Some(b'\\') => {
                    if self.peek().is_some() {
                        self.pos += 1;
                    }
                }
                Some(q @ (b'\'' | b'"')) => {
                    // self.next() consumed the quote; step back so the
                    // scanners below start on it.
                    self.pos -= 1;
                    if q == b'\'' {
                        self.scan_quoted(b'\'', start)?;
                    } else {
                        self.scan_quoted_double()?;
                    }
                }
                Some(b'`') => {
                    // Nested backquotes: scan to the match (no nesting level).
                    let save = self.pos - 1;
                    self.pos = save;
                    let mut cooked = String::new();
                    let mut exps = Vec::new();
                    self.scan_backquote(&mut cooked, &mut exps)?;
                }
                Some(b'$') => match self.peek() {
                    Some(b'(') => {
                        self.pos += 1;
                        depth += 1;
                    }
                    Some(b'{') => {
                        self.pos += 1;
                        let _ = self.scan_balanced(b'{', b'}', start)?;
                    }
                    _ => {}
                },
                Some(b'#') => {
                    // Comment inside substitution: skip to newline.
                    while matches!(self.peek(), Some(c) if c != b'\n') {
                        self.pos += 1;
                    }
                }
                Some(b'<') if self.peek() == Some(b'<') => {
                    // Possible heredoc inside the substitution: skip its body
                    // so parens inside cannot unbalance the scan.
                    if !self.skip_heredoc_body() {
                        self.pos += 1;
                    }
                }
                Some(c) if c == open && open != close => {
                    depth += 1;
                }
                Some(c) if c == close => {
                    depth -= 1;
                }
                Some(_) => {}
            }
        }
        Ok(self.pos)
    }

    /// Scan a double-quoted span starting at the quote.
    fn scan_quoted_double(&mut self) -> Result<(), LexError> {
        debug_assert_eq!(self.peek(), Some(b'"'));
        let start = self.pos;
        self.pos += 1;
        loop {
            match self.next() {
                None => return Err(LexError::new("unterminated double quote", start)),
                Some(b'"') => return Ok(()),
                Some(b'\\') => {
                    if self.peek().is_some() {
                        self.pos += 1;
                    }
                }
                Some(b'$') => match self.peek() {
                    Some(b'(') => {
                        self.pos += 1;
                        let _ = self.scan_balanced(b'(', b')', start)?;
                    }
                    Some(b'{') => {
                        self.pos += 1;
                        let _ = self.scan_balanced(b'{', b'}', start)?;
                    }
                    _ => {}
                },
                Some(b'`') => {
                    let mut cooked = String::new();
                    let mut exps = Vec::new();
                    self.pos -= 1;
                    self.scan_backquote(&mut cooked, &mut exps)?;
                }
                Some(_) => {}
            }
        }
    }

    /// After seeing `<<` inside a substitution scan, skip a heredoc body
    /// when one follows. Returns true when a body was skipped.
    fn skip_heredoc_body(&mut self) -> bool {
        let save = self.pos;
        // We are positioned at the second `<`.
        self.pos += 1;
        let strip_tabs = if self.peek() == Some(b'-') {
            self.pos += 1;
            true
        } else {
            false
        };
        if self.peek() == Some(b'<') {
            // `<<<` herestring: no body follows.
            self.pos = save;
            return false;
        }
        // Skip blanks, read the delimiter word (honor simple quotes).
        while matches!(self.peek(), Some(b' ') | Some(b'\t')) {
            self.pos += 1;
        }
        let mut delim = String::new();
        let mut quoted = false;
        match self.peek() {
            Some(b'\'') | Some(b'"') => {
                quoted = true;
                let q = self.next().unwrap_or(0);
                while let Some(c) = self.next() {
                    if c == q {
                        break;
                    }
                    delim.push(c as char);
                }
            }
            _ => {
                while let Some(c) = self.peek() {
                    if c == b'\n' || c == b' ' || c == b'\t' || c == b';' || c == b'&' || c == b'|' {
                        break;
                    }
                    delim.push(c as char);
                    self.pos += 1;
                }
            }
        }
        if delim.is_empty() {
            self.pos = save;
            return false;
        }
        let _ = (quoted, strip_tabs);
        // Skip to the end of the current line, then lines until delimiter.
        while matches!(self.peek(), Some(c) if c != b'\n') {
            self.pos += 1;
        }
        loop {
            if self.at_end() {
                return true;
            }
            // Consume the newline ending the previous line.
            if self.peek() == Some(b'\n') {
                self.pos += 1;
            }
            let line_start = self.pos;
            while matches!(self.peek(), Some(c) if c != b'\n') {
                self.pos += 1;
            }
            let mut line = &self.input[line_start..self.pos];
            if strip_tabs {
                while line.first() == Some(&b'\t') {
                    line = &line[1..];
                }
            }
            if line == delim.as_bytes() {
                return true;
            }
        }
    }
}

/// True for bytes that start an operator token.
fn is_operator_start(c: u8) -> bool {
    matches!(
        c,
        b';' | b'&' | b'|' | b'<' | b'>' | b'(' | b')' | b'{' | b'}' | b'!'
    )
}

/// True for bytes that terminate an unquoted word.
fn is_word_break(c: u8) -> bool {
    matches!(
        c,
        b' ' | b'\t'
            | b'\n'
            | b';'
            | b'&'
            | b'|'
            | b'('
            | b')'
            | b'<'
            | b'>'
    )
}

/// True for `[A-Za-z_]`.
fn is_name_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}

/// True for `[A-Za-z0-9_]`.
fn is_name_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan_all(input: &str) -> Result<Vec<Token>, LexError> {
        let mut lexer = Lexer::new(input);
        let mut tokens = Vec::new();
        loop {
            let tok = lexer.next_token()?;
            let done = matches!(tok, Token::Eof);
            tokens.push(tok);
            if done {
                break;
            }
        }
        Ok(tokens)
    }

    fn word_texts(tokens: &[Token]) -> Vec<&str> {
        tokens
            .iter()
            .filter_map(|t| match t {
                Token::Word(w) => Some(w.cooked.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn simple_words_and_operators() {
        let tokens = scan_all("echo foo && bar").unwrap();
        assert_eq!(word_texts(&tokens), vec!["echo", "foo", "bar"]);
        let ops: Vec<&str> = tokens
            .iter()
            .filter_map(|t| match t {
                Token::Op(k, _) => Some(k.text()),
                _ => None,
            })
            .collect();
        assert_eq!(ops, vec!["&&"]);
    }

    #[test]
    fn quotes_are_removed_but_spans_cover_raw() {
        let tokens = scan_all("echo \"a b\" 'c d'").unwrap();
        let words: Vec<&RawWord> = tokens
            .iter()
            .filter_map(|t| match t {
                Token::Word(w) => Some(w),
                _ => None,
            })
            .collect();
        assert_eq!(words.len(), 3);
        assert_eq!(words[1].cooked, "a b");
        assert!(words[1].quoted);
        // Raw span covers the quotes: `"a b"` is 5 bytes at 5..10.
        assert_eq!(words[1].span.len(), 5);
        assert_eq!(words[2].cooked, "c d");
    }

    #[test]
    fn escapes_and_continuations() {
        let tokens = scan_all("echo a\\ b c\\\nd").unwrap();
        assert_eq!(word_texts(&tokens), vec!["echo", "a b", "cd"]);
    }

    #[test]
    fn command_substitution_span_and_inner() {
        let tokens = scan_all("echo $(foo bar)").unwrap();
        let words: Vec<&RawWord> = tokens
            .iter()
            .filter_map(|t| match t {
                Token::Word(w) => Some(w),
                _ => None,
            })
            .collect();
        assert_eq!(words.len(), 2);
        assert_eq!(words[1].cooked, "$(foo bar)");
        assert_eq!(words[1].expansions.len(), 1);
        match &words[1].expansions[0] {
            RawExpansion::CmdSubst { span, inner } => {
                assert_eq!(&"echo $(foo bar)".as_bytes()[span.start..span.end], b"$(foo bar)");
                assert_eq!(&"echo $(foo bar)".as_bytes()[inner.start..inner.end], b"foo bar");
            }
            other => panic!("expected cmdsubst, got {other:?}"),
        }
    }

    #[test]
    fn nested_substitutions_balance() {
        let tokens = scan_all("echo $(foo $(bar) baz)").unwrap();
        let words: Vec<&RawWord> = tokens
            .iter()
            .filter_map(|t| match t {
                Token::Word(w) => Some(w),
                _ => None,
            })
            .collect();
        assert_eq!(words[1].cooked, "$(foo $(bar) baz)");
    }

    #[test]
    fn paren_inside_quotes_does_not_close() {
        let tokens = scan_all("echo $(foo \")\" bar)").unwrap();
        let words: Vec<&RawWord> = tokens
            .iter()
            .filter_map(|t| match t {
                Token::Word(w) => Some(w),
                _ => None,
            })
            .collect();
        assert_eq!(words[1].cooked, "$(foo \")\" bar)");
    }

    #[test]
    fn parameters_and_tilde() {
        let tokens = scan_all("echo $HOME ${x:-d} ~/bin ~root/a $1 $@").unwrap();
        let words: Vec<&RawWord> = tokens
            .iter()
            .filter_map(|t| match t {
                Token::Word(w) => Some(w),
                _ => None,
            })
            .collect();
        // echo, $HOME, ${x:-d}, ~/bin, ~root/a, $1, $@
        assert_eq!(words.len(), 7);
        assert!(matches!(
            words[1].expansions[..],
            [RawExpansion::Param { .. }]
        ));
        assert!(matches!(
            words[3].expansions[..],
            [RawExpansion::Tilde { .. }]
        ));
        assert_eq!(words[5].cooked, "$1");
    }

    #[test]
    fn process_substitution() {
        let tokens = scan_all("cat <(foo) >(bar)").unwrap();
        let words: Vec<&RawWord> = tokens
            .iter()
            .filter_map(|t| match t {
                Token::Word(w) => Some(w),
                _ => None,
            })
            .collect();
        assert_eq!(words.len(), 3);
        assert!(matches!(
            words[1].expansions[..],
            [RawExpansion::ProcSubst { .. }]
        ));
    }

    #[test]
    fn longest_match_operators() {
        let tokens = scan_all("a &>>b ;; c ;;& d ;& e").unwrap();
        let ops: Vec<&str> = tokens
            .iter()
            .filter_map(|t| match t {
                Token::Op(k, _) => Some(k.text()),
                _ => None,
            })
            .collect();
        assert_eq!(ops, vec!["&>>", ";;", ";;&", ";&"]);
    }

    #[test]
    fn comments_and_newlines() {
        let tokens = scan_all("echo hi # comment\nfoo").unwrap();
        assert_eq!(word_texts(&tokens), vec!["echo", "hi", "foo"]);
        assert!(tokens
            .iter()
            .any(|t| matches!(t, Token::Newline(_))));
    }

    #[test]
    fn heredoc_body_does_not_unbalance() {
        // The `)` inside the heredoc body must not close the substitution.
        let tokens = scan_all("echo $(cat <<EOF\nfoo)bar\nEOF\n)").unwrap();
        let words: Vec<&RawWord> = tokens
            .iter()
            .filter_map(|t| match t {
                Token::Word(w) => Some(w),
                _ => None,
            })
            .collect();
        assert_eq!(words.len(), 2);
        assert!(words[1].cooked.starts_with("$(cat"));
    }

    #[test]
    fn unterminated_constructs_error() {
        assert!(scan_all("echo \"abc").is_err());
        assert!(scan_all("echo 'abc").is_err());
        assert!(scan_all("echo $(foo").is_err());
        assert!(scan_all("echo `foo").is_err());
    }
}
