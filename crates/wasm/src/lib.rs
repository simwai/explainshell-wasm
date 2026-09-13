//! WASM bindings for explainshell.
//!
//! Exposes the matcher over wasm-bindgen (wasm32-unknown-unknown) so the
//! npm TypeScript layer loads one artifact in Node.js and browsers.
//! Results cross the boundary as JSON strings parsed and validated by
//! src/runtime/utils.ts.

use explainshell_data::ManpageData;
use explainshell_match::explain_command;
use explainshell_parse::{AstNode, parse};
use wasm_bindgen::prelude::*;

fn js_err(message: String) -> JsValue {
    JsValue::from_str(&message)
}

/// Install the console.error panic hook. Called automatically by
/// Explainshell::new; exported for hosts that want it earlier.
#[wasm_bindgen]
pub fn init_panic_hook() {
    console_error_panic_hook::set_once();
}

/// Bundle schema version this module understands.
#[wasm_bindgen]
pub fn bundle_version() -> u32 {
    explainshell_data::BUNDLE_VERSION
}

/// An explainshell engine backed by one manpage data bundle.
#[wasm_bindgen]
pub struct Explainshell {
    data: ManpageData,
}

#[wasm_bindgen]
impl Explainshell {
    /// Load a MessagePack data bundle (see scripts/export_wasm_data.py).
    #[wasm_bindgen(constructor)]
    pub fn new(data: &[u8]) -> Result<Explainshell, JsValue> {
        console_error_panic_hook::set_once();
        ManpageData::from_msgpack(data)
            .map(|data| Explainshell { data })
            .map_err(|e| js_err(e.to_string()))
    }

    /// Explain a shell command. Parses the command into an AST, matches it
    /// against the manpage data, and returns the ExplainResult as JSON.
    pub fn explain(&self, command: &str) -> Result<String, JsValue> {
        let ast = parse(command, true, Some(1)).map_err(|e| js_err(e.to_string()))?;
        let result = explain_ast_nodes(&ast, &self.data).map_err(|e| js_err(e.to_string()))?;
        serde_json::to_string(&result).map_err(|e| js_err(e.to_string()))
    }

    /// Explain from a pre-parsed AST JSON string (bashlex-compatible).
    pub fn explain_ast(&self, ast_json: &str) -> Result<String, JsValue> {
        let ast: Vec<AstNode> =
            serde_json::from_str(ast_json).map_err(|e| js_err(e.to_string()))?;
        let result = explain_ast_nodes(&ast, &self.data).map_err(|e| js_err(e.to_string()))?;
        serde_json::to_string(&result).map_err(|e| js_err(e.to_string()))
    }

    /// Number of manpages in the loaded bundle.
    pub fn manpage_count(&self) -> usize {
        self.data.manpage_count()
    }
}

fn explain_ast_nodes(
    ast: &[AstNode],
    data: &ManpageData,
) -> Result<explainshell_core::ExplainResult, explainshell_match::MatchError> {
    let mut result = explainshell_core::ExplainResult {
        groups: Vec::new(),
        expansions: Vec::new(),
    };
    for node in ast {
        explain_node(node, data, &mut result)?;
    }
    Ok(result)
}

fn explain_node(
    node: &AstNode,
    data: &ManpageData,
    result: &mut explainshell_core::ExplainResult,
) -> Result<(), explainshell_match::MatchError> {
    match node {
        AstNode::List { parts, .. } | AstNode::Pipeline { parts, .. } => {
            for part in parts {
                explain_node(part, data, result)?;
            }
        }
        AstNode::Command { parts, .. } => {
            let words = parts
                .iter()
                .filter_map(|p| match p {
                    AstNode::Word(w) | AstNode::Assignment(w) => Some(w),
                    _ => None,
                })
                .collect::<Vec<_>>();
            if let Some(first) = words.first() {
                let cmd = explain_command(first.text.as_str(), data)?;
                result.groups.extend(cmd.groups);
                result.expansions.extend(cmd.expansions);
            }
        }
        AstNode::Compound { list, .. } => {
            for part in list {
                explain_node(part, data, result)?;
            }
        }
        AstNode::Keyword { parts, .. } | AstNode::Function { parts, .. } => {
            for part in parts {
                explain_node(part, data, result)?;
            }
        }
        AstNode::Redirect { output, .. } => {
            if let explainshell_parse::RedirectTarget::Word(w) = output {
                if let Ok(cmd) = explain_command(w.text.as_str(), data) {
                    result.groups.extend(cmd.groups);
                    result.expansions.extend(cmd.expansions);
                }
            }
        }
        _ => {}
    }
    Ok(())
}
