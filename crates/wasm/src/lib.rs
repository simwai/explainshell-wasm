//! WASM bindings for explainshell.
//!
//! Exposes the matcher over wasm-bindgen (wasm32-unknown-unknown) so the
//! npm TypeScript layer loads one artifact in Node.js and browsers.
//! Results cross the boundary as JSON strings parsed and validated by
//! src/runtime/utils.ts.

use explainshell_data::ManpageData;
use explainshell_match::explain_command;
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

    /// Explain a shell command. Returns the ExplainResult as a JSON string.
    pub fn explain(&self, command: &str) -> Result<String, JsValue> {
        let result = explain_command(command, &self.data).map_err(|e| js_err(e.to_string()))?;
        serde_json::to_string(&result).map_err(|e| js_err(e.to_string()))
    }

    /// Number of manpages in the loaded bundle.
    pub fn manpage_count(&self) -> usize {
        self.data.manpage_count()
    }
}
