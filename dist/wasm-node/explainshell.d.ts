/* tslint:disable */
/* eslint-disable */

/**
 * An explainshell engine backed by one manpage data bundle.
 */
export class Explainshell {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Explain from a pre-parsed AST JSON string (bashlex-compatible).
     */
    explain_ast(ast_json: string): string;
    /**
     * Explain a shell command. Parses the command into an AST, matches it
     * against the manpage data, and returns the ExplainResult as JSON.
     */
    explain(command: string): string;
    /**
     * Number of manpages in the loaded bundle.
     */
    manpage_count(): number;
    /**
     * Load a MessagePack data bundle (see scripts/export_wasm_data.py).
     */
    constructor(data: Uint8Array);
}

/**
 * Bundle schema version this module understands.
 */
export function bundle_version(): number;

/**
 * Install the console.error panic hook. Called automatically by
 * Explainshell::new; exported for hosts that want it earlier.
 */
export function init_panic_hook(): void;
