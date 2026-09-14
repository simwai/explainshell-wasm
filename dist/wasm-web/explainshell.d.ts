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

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_explainshell_free: (a: number, b: number) => void;
    readonly bundle_version: () => number;
    readonly explainshell_explain: (a: number, b: number, c: number, d: number) => void;
    readonly explainshell_explain_ast: (a: number, b: number, c: number, d: number) => void;
    readonly explainshell_manpage_count: (a: number) => number;
    readonly explainshell_new: (a: number, b: number, c: number) => void;
    readonly init_panic_hook: () => void;
    readonly __wbindgen_export: (a: number, b: number, c: number) => void;
    readonly __wbindgen_export2: (a: number, b: number) => number;
    readonly __wbindgen_export3: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_add_to_stack_pointer: (a: number) => number;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
