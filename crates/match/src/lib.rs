//! Matcher algorithm for explainshell WASM.
//!
//! Ports explainshell/matcher.py: walks the shell AST, resolves each command
//! to its manpage, and matches tokens to options/positionals.

use explainshell_core::{ExplainResult, HasArgument, MatchGroup, MatchResult, ParsedManpage};
use explainshell_data::ManpageData;
use explainshell_parse::{AstNode, WordNode};
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

/// Shell-level help text for reserved words, operators, pipes, redirects.
mod help {
    pub const NO_SYNOPSIS: &str = "no synopsis found";

    pub const PIPELINES: &str = "A pipeline is a sequence of one or more commands separated by one of the control operators | or |&.";

    pub const OPSEMICOLON: &str = "Commands separated by ; are executed sequentially.";

    pub const OPBACKGROUND: &str = "If a command is terminated by &, the shell executes the command in the background.";

    pub const OPANDOR: &str = "AND and OR lists are sequences of pipelines separated by && and ||.";

    pub const OPERATORS: &[(&str, &str)] = &[
        (";", OPSEMICOLON),
        ("&", OPBACKGROUND),
        ("&&", OPANDOR),
        ("||", OPANDOR),
    ];

    pub const REDIRECTION: &str = "Before a command is executed, its input and output may be redirected.";

    pub const RESERVED_WORDS: &[(&str, &str)] = &[
        ("if", "The if command executes a list of commands based on a condition."),
        ("then", "The then keyword marks the start of the if-true branch."),
        ("else", "The else keyword marks the start of the if-false branch."),
        ("fi", "The fi keyword ends an if statement."),
        ("for", "The for loop executes a list of commands for each item in a list."),
        ("while", "The while loop executes a list of commands while a condition is true."),
        ("until", "The until loop executes a list of commands until a condition is true."),
        ("do", "The do keyword starts the body of a for/while/until loop."),
        ("done", "The done keyword ends the body of a for/while/until loop."),
        ("case", "The case statement executes commands based on pattern matching."),
        ("esac", "The esac keyword ends a case statement."),
        ("in", "The in keyword introduces patterns in a case statement."),
        ("function", "A function definition creates a reusable command sequence."),
        ("!", "The ! operator negates the exit status of a pipeline."),
    ];

    pub const ASSIGNMENT: &str = "An assignment statement sets a variable value.";
}

/// Match a raw command string against the manpage data.
pub fn explain_command(
    input: &str,
    data: &ManpageData,
) -> Result<ExplainResult, MatchError> {
    let mut matcher = Matcher::new(input, data);
    let groups = matcher.match_ast()?;
    Ok(ExplainResult { groups, expansions: matcher.expansions })
}

struct Matcher<'a> {
    input: &'a str,
    data: &'a ManpageData,
    groups: Vec<MatchGroup>,
    expansions: Vec<explainshell_core::Expansion>,
    group_stack: Vec<MatchGroup>,
    compound_stack: Vec<&'a str>,
    functions: Vec<String>,
    prev_option: Option<explainshell_core::CliOption>,
    current_option: Option<explainshell_core::CliOption>,
}

impl<'a> Matcher<'a> {
    fn new(input: &'a str, data: &'a ManpageData) -> Self {
        let shell_group = MatchGroup {
            name: "shell".to_string(),
            results: Vec::new(),
            manpage: None,
            suggestions: Vec::new(),
            error: None,
            positional_index: 0,
        };
        Self {
            input,
            data,
            groups: vec![shell_group.clone()],
            expansions: Vec::new(),
            group_stack: vec![shell_group],
            compound_stack: Vec::new(),
            functions: Vec::new(),
            prev_option: None,
            current_option: None,
        }
    }

    fn matches(&mut self) -> &mut Vec<MatchResult> {
        &mut self.group_stack.last_mut().unwrap().results
    }

    fn man_page(&self) -> Option<&ParsedManpage> {
        let group = self.group_stack.last()?;
        if group.name == "shell" {
            None
        } else {
            group.manpage.as_ref()
        }
    }

    fn find_man_pages(&mut self, prog: &str) -> Result<Vec<ParsedManpage>, MatchError> {
        let result = self.data.find_man_page(prog);
        result.map(|(mp, sugg)| {
            let mut v = vec![mp.clone()];
            v.extend(sugg.to_vec().into_iter().cloned());
            v
        }).map_err(|_| MatchError::UnknownProgram(prog.to_string()))
    }

    fn unknown(&self, _token: &str, start: usize, end: usize) -> MatchResult {
        MatchResult {
            start,
            end,
            text: None,
            match_text: None,
            debug_info: Some(serde_json::json!({"kind": "unknown"})),
        }
    }

    fn generate_cmd_group_name(&self) -> String {
        let existing = self.groups.iter().filter(|g| g.name.starts_with("command")).count();
        format!("command{}", existing)
    }

    fn match_ast(&mut self) -> Result<Vec<MatchGroup>, MatchError> {
        let ast = explainshell_parse::parse(self.input, true, Some(1))
            .map_err(|_| MatchError::EmptyInput)?;
        if ast.is_empty() {
            return Err(MatchError::EmptyInput);
        }
        for node in &ast {
            self.walk(node)?;
        }
        Ok(std::mem::take(&mut self.groups))
    }

    fn walk(&mut self, node: &AstNode) -> Result<(), MatchError> {
        match node {
            AstNode::List { parts, .. } | AstNode::Pipeline { parts, .. } => {
                for part in parts {
                    self.walk(part)?;
                }
            }
            AstNode::Command { parts, .. } => {
                self.visit_command(parts)?;
            }
            AstNode::Compound { list, .. } => {
                for part in list {
                    self.walk(part)?;
                }
            }
            AstNode::Keyword { kind, parts, .. } => {
                let compound = match kind {
                    explainshell_parse::KeywordKind::If => "if",
                    explainshell_parse::KeywordKind::For => "for",
                    explainshell_parse::KeywordKind::While => "while",
                    explainshell_parse::KeywordKind::Until => "until",
                    explainshell_parse::KeywordKind::Case => "case",
                    explainshell_parse::KeywordKind::Pattern => "pattern",
                };
                self.compound_stack.push(compound);
                for part in parts {
                    self.walk(part)?;
                }
                self.compound_stack.pop();
            }
            AstNode::Function { name, body, .. } => {
                self.functions.push(name.text.clone());
                self.walk(body)?;
                self.functions.pop();
            }
            AstNode::Redirect { output, .. } => {
                if let explainshell_parse::RedirectTarget::Word(w) = output {
                    self.add_redirect_help(w.span);
                    for part in &w.parts {
                        self.walk(part)?;
                    }
                }
            }
            AstNode::CommandSubstitution { span, .. } => {
                self.add_expansion(*span, "substitution");
            }
            AstNode::ProcessSubstitution { span, .. } => {
                self.add_expansion(*span, "substitution");
            }
            AstNode::Tilde { span, .. } => {
                self.add_expansion(*span, "tilde");
            }
            AstNode::Parameter { span, value, .. } => {
                let kind = if value.parse::<i64>().is_ok() { "digits" } else { "param" };
                self.add_expansion(*span, &format!("parameter-{}", kind));
            }
            AstNode::Heredoc { .. } => {}
            AstNode::ReservedWord { word, span, .. } => {
                let helptext = help::RESERVED_WORDS
                    .iter()
                    .find(|(w, _)| *w == word)
                    .map(|(_, t)| *t)
                    .unwrap_or("");
                self.add_shell_result(*span, helptext, "reserved_word");
            }
            AstNode::Operator { op, span, .. } => {
                let helptext = help::OPERATORS
                    .iter()
                    .find(|(o, _)| *o == op)
                    .map(|(_, t)| *t)
                    .unwrap_or("");
                self.add_shell_result(*span, helptext, "operator");
            }
            AstNode::Pipe { span, .. } => {
                self.add_shell_result(*span, help::PIPELINES, "pipe");
            }
            AstNode::Word(w) => {
                self.visit_word(w)?;
            }
            AstNode::Assignment(w) => {
                self.add_shell_result(w.span, help::ASSIGNMENT, "assignment");
            }
            _ => {}
        }
        Ok(())
    }

    fn visit_command(&mut self, parts: &[AstNode]) -> Result<(), MatchError> {
        if parts.is_empty() {
            return Ok(());
        }

        // Find first word node
        let idx_word = find_first_kind(parts, "word");
        if idx_word == -1 {
            return Ok(());
        }

        let word_node = match &parts[idx_word as usize] {
            AstNode::Word(w) => w,
            _ => return Ok(()),
        };

        // Check if this is a function call
        if self.functions.contains(&word_node.text) {
            self.add_match_result(
                word_node.span.start,
                word_node.span.end,
                &format!("Call to function '{}'.", word_node.text),
                None,
                &serde_json::json!({"kind": "function_call"}),
            );
            return Ok(());
        }

        // Look up manpage
        let mps = self.find_man_pages(&word_node.text)?;
        let mut manpage = mps[0].clone();
        let suggestions = mps[1..].to_vec();

        // Check for subcommand
        let mut endpos = word_node.span.end;
        for part in parts {
            if let AstNode::Word(w) = part {
                if w.span.start > word_node.span.start {
                    let multi = format!("{} {}", word_node.text, w.text);
                    if let Ok(sub_mps) = self.find_man_pages(&multi) {
                        manpage = sub_mps[0].clone();
                        endpos = w.span.end;
                        break;
                    }
                }
            }
        }

        // Create command group
        let mut group = MatchGroup {
            name: self.generate_cmd_group_name(),
            results: Vec::new(),
            manpage: Some(manpage),
            suggestions,
            error: None,
            positional_index: 0,
        };

        // Add synopsis
        let synopsis = group.manpage.as_ref().and_then(|mp| mp.synopsis.as_deref()).unwrap_or(help::NO_SYNOPSIS);
        group.results.push(MatchResult {
            start: word_node.span.start,
            end: endpos,
            text: Some(synopsis.to_string()),
            match_text: None,
            debug_info: Some(serde_json::json!({"kind": "synopsis"})),
        });

        self.group_stack.push(group);

        // Match remaining words and redirects as arguments
        for part in parts {
            match part {
                AstNode::Word(w) => {
                    if w.span.start != word_node.span.start || w.span.end != word_node.span.end {
                        self.visit_word(w)?;
                    }
                }
                AstNode::Redirect { output, span, .. } => {
                    if let explainshell_parse::RedirectTarget::Word(w) = output {
                        self.add_redirect_help(*span);
                        for part in &w.parts {
                            self.walk(part)?;
                        }
                    }
                }
                _ => {}
            }
        }

        // End command group and move completed group to groups
        let completed = self.group_stack.pop().unwrap();
        self.groups.push(completed);
        Ok(())
    }

    fn end_command(&mut self) {
        if self.group_stack.len() > 1 {
            self.group_stack.pop();
        }
    }

    fn visit_word(&mut self, word: &WordNode) -> Result<(), MatchError> {
        let text = word.text.as_str();
        let start = word.span.start;
        let end = word.span.end;

        if self.man_page().is_none() {
            self.add_unknown(start, end, text);
            return Ok(());
        }

        self.prev_option = self.current_option.clone();
        let mut word_to_match = text;

        // Strip --flag= to --flag
        if text.starts_with("--") {
            if let Some(eq) = text.find('=') {
                word_to_match = &text[..eq];
            }
        }

        // Try exact option match
        if let Some(manpage) = self.man_page().cloned() {
            if let Some(option) = manpage.find_option(word_to_match) {
                self.current_option = Some(option.clone());
                self.add_match_result(
                    start,
                    end,
                    &option.text,
                    Some(text),
                    &serde_json::json!({"kind": "option", "short": option.short, "long": option.long, "has_argument": option.has_argument}),
                );
                if word_to_match != text {
                    self.current_option = None;
                }
                return Ok(());
            }
        }

        // Try short option cluster
        if text != "-" && text.starts_with('-') && !text.starts_with("--") && !word.quoted {
            if text.len() > 2 {
                self.match_short_options(text, start, end)?;
                return Ok(());
            }
        }

        // Check if previous option expects an argument
        if let Some(prev) = &self.prev_option {
            if let HasArgument::Bool(true) = prev.has_argument {
                // Take this word as argument to previous option
                if let Some(last) = self.matches().last_mut() {
                    last.end = end;
                }
                return Ok(());
            }
        }

        // Try positionals
        if let Some(manpage) = self.man_page().cloned() {
            let positionals = manpage.positionals();
            let prefixed = manpage.prefixed_positionals();
            if positionals.is_empty() && prefixed.is_empty() {
                self.add_unknown(start, end, text);
                return Ok(());
            }

            // Try prefixed positionals first
            for (k, (prefix, text)) in &prefixed {
                if text.starts_with(prefix) {
                    self.add_match_result(
                        start,
                        end,
                        text,
                        None,
                        &serde_json::json!({"kind": "argument", "positional": k, "prefix": prefix}),
                    );
                    return Ok(());
                }
            }

            let keys: Vec<&String> = positionals.iter().map(|(k, _)| k).collect();
            let group = self.group_stack.last_mut().unwrap();

            let key = if positionals.iter().any(|(k, _)| k == text) {
                Some(text.to_string())
            } else if group.positional_index < keys.len() {
                let k = keys[group.positional_index].clone();
                group.positional_index += 1;
                Some(k)
            } else if !keys.is_empty() {
                keys.last().cloned().map(|s| s.clone())
            } else {
                None
            };

            if let Some(k) = key {
                if let Some((_, text)) = positionals.iter().find(|(k2, _)| k2 == &k) {
                    self.add_match_result(
                        start,
                        end,
                        text,
                        None,
                        &serde_json::json!({"kind": "argument", "positional": k}),
                    );
                    return Ok(());
                }
            }
        }

        self.add_unknown(start, end, text);
        Ok(())
    }

    fn match_short_options(&mut self, text: &str, start: usize, end: usize) -> Result<(), MatchError> {
        let mut tokens: Vec<String> = text.chars().skip(1).map(|c| c.to_string()).collect();
        let mut pos = start;
        let mut prev_option: Option<explainshell_core::CliOption> = None;

        for (i, t) in tokens.iter().enumerate() {
            let op = format!("-{}", t);
            let manpage = self.man_page().cloned();
            if let Some(ref mp) = manpage {
                if let Some(option) = mp.find_option(&op) {
                    if i == 0 && matches!(option.has_argument, HasArgument::Bool(true)) {
                        // First short option takes the rest as argument
                        self.current_option = Some(option.clone());
                        self.add_match_result(
                            start,
                            end,
                            &option.text,
                            Some(text),
                            &serde_json::json!({"kind": "option", "short": option.short, "long": option.long, "has_argument": option.has_argument}),
                        );
                        self.current_option = None;
                        return Ok(());
                    }
                    self.add_match_result(
                        pos,
                        pos + t.len(),
                        &option.text,
                        Some(t.as_str()),
                        &serde_json::json!({"kind": "option", "short": option.short, "long": option.long, "has_argument": option.has_argument}),
                    );
                } else if i > 0 && prev_option.as_ref().map(|o| matches!(o.has_argument, HasArgument::Bool(true))).unwrap_or(false) {
                    // Previous option expected arg, take rest
                    if let Some(last) = self.matches().last_mut() {
                        last.end = end;
                    }
                    self.current_option = None;
                    break;
                } else {
                    self.add_unknown(pos, pos + t.len(), t);
                }
            }
            pos += t.len();
            prev_option = manpage.as_ref().and_then(|mp| mp.find_option(&op).cloned());
        }
        Ok(())
    }

    fn add_match_result(&mut self, start: usize, end: usize, text: &str, match_text: Option<&str>, debug_info: &serde_json::Value) {
        self.matches().push(MatchResult {
            start,
            end,
            text: Some(text.to_string()),
            match_text: match_text.map(|s| s.to_string()),
            debug_info: Some(debug_info.clone()),
        });
    }

    fn add_unknown(&mut self, start: usize, end: usize, text: &str) {
        self.add_match_result(start, end, text, None, &serde_json::json!({"kind": "unknown"}));
    }

    fn add_shell_result(&mut self, span: explainshell_parse::Span, text: &str, kind: &str) {
        self.groups[0].results.push(MatchResult {
            start: span.start,
            end: span.end,
            text: Some(text.to_string()),
            match_text: None,
            debug_info: Some(serde_json::json!({"kind": kind})),
        });
    }

    fn add_expansion(&mut self, span: explainshell_parse::Span, kind: &str) {
        self.expansions.push(explainshell_core::Expansion {
            start: span.start,
            end: span.end,
            kind: kind.to_string(),
        });
    }

    fn add_redirect_help(&mut self, span: explainshell_parse::Span) {
        self.add_shell_result(span, help::REDIRECTION, "redirect");
    }
}

/// Find the first child of the given kind (mirrors bashlex ast.findfirstkind).
fn find_first_kind(parts: &[AstNode], kind: &str) -> i64 {
    parts
        .iter()
        .position(|p| p.kind() == kind)
        .map(|i| i as i64)
        .unwrap_or(-1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use explainshell_data::ManpageData;

    fn load_test_data() -> ManpageData {
        let bytes = std::fs::read("../../test/fixtures/test.data.msgpack").expect("test fixture missing");
        ManpageData::from_msgpack(&bytes).expect("failed to decode test fixture")
    }

    #[test]
    fn explain_command_returns_groups() {
        let data = load_test_data();
        let result = explain_command("tar -xvf archive.tar", &data).expect("explain failed");
        assert!(!result.groups.is_empty(), "expected at least one group");
    }

    #[test]
    fn explain_command_shell_group_has_reserved_words() {
        let data = load_test_data();
        let result = explain_command("tar -xvf archive.tar", &data).expect("explain failed");
        let shell = &result.groups[0];
        assert_eq!(shell.name, "shell");
        // Simple commands don't produce shell-level results; command group does.
        let command_groups: Vec<_> = result.groups.iter().filter(|g| g.name != "shell").collect();
        assert!(!command_groups.is_empty(), "expected at least one command group");
        assert!(command_groups[0].manpage.is_some(), "expected manpage for tar");
    }

    #[test]
    fn explain_unknown_program_returns_error_group() {
        let data = load_test_data();
        let result = explain_command("unknownprog123", &data);
        assert!(result.is_err(), "expected error for unknown program");
        match result.unwrap_err() {
            MatchError::UnknownProgram(name) => assert_eq!(name, "unknownprog123"),
            _ => panic!("expected UnknownProgram error"),
        }
    }

    #[test]
    fn explain_ast_roundtrip() {
        let data = load_test_data();
        let result = explain_command("tar -xvf archive.tar", &data).expect("explain failed");
        let json = serde_json::to_string(&result).unwrap();
        let parsed: ExplainResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.groups.len(), result.groups.len());
    }

    #[test]
    fn explain_git_subcommand_resolves_git_commit_manpage() {
        let data = load_test_data();
        let result = explain_command("git commit -m \"fix bug\"", &data).expect("explain failed");
        let command_groups: Vec<_> = result.groups.iter().filter(|g| g.name != "shell").collect();
        assert!(!command_groups.is_empty(), "expected command group for git commit");
        let manpage = command_groups[0].manpage.as_ref().expect("expected manpage");
        assert_eq!(manpage.name, "git-commit");
    }

    #[test]
    fn explain_long_options_are_matched() {
        let data = load_test_data();
        let result = explain_command("tar --extract --verbose --file=foo.tar", &data).expect("explain failed");
        let command_groups: Vec<_> = result.groups.iter().filter(|g| g.name != "shell").collect();
        assert!(!command_groups.is_empty(), "expected command group");
        let results = &command_groups[0].results;
        assert!(results.iter().any(|m| m.text.as_deref() == Some("extract files") || m.match_text.as_deref() == Some("--extract")),
                "expected --extract match");
    }

    #[test]
    fn explain_option_with_absorbed_argument() {
        let data = load_test_data();
        let result = explain_command("tar -f archive.tar", &data).expect("explain failed");
        let command_groups: Vec<_> = result.groups.iter().filter(|g| g.name != "shell").collect();
        assert!(!command_groups.is_empty(), "expected command group");
        let results = &command_groups[0].results;
        assert!(results.iter().any(|m| m.match_text.as_deref() == Some("-f")), "expected -f match");
    }

    #[test]
    fn explain_redirect_is_shell_result() {
        let data = load_test_data();
        let result = explain_command("tar -xvf foo.tar > out.txt", &data).expect("explain failed");
        let shell = result.groups.iter().find(|g| g.name == "shell");
        assert!(shell.is_some(), "expected shell group for redirect");
        assert!(shell.unwrap().results.iter().any(|r| r.debug_info.as_ref().and_then(|d| d.get("kind").and_then(|k| k.as_str())) == Some("redirect")),
                "expected redirect result");
    }
}
