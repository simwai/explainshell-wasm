import type { ExplainResult } from '../types.js'

/**
 * Shape of the wasm-bindgen glue module (nodejs and web targets).
 * Loaded dynamically so typechecking never depends on build output;
 * validated at runtime by isWasmGlueModule.
 */
export interface WasmGlueModule {
  /** Command engine class */
  Explainshell: new (
    data: Uint8Array
  ) => WasmExplainshell
  /** Bundle schema version (optional) */
  bundle_version?: () => number
  /** Install the console.error panic hook (optional, also done by new) */
  init_panic_hook?: () => void
  /** Web-target initializer: instantiate with the _bg.wasm URL */
  default?: (
    input?: string | URL | Response | BufferSource | WebAssembly.Module
  ) => Promise<unknown>
}

/** Minimal surface of the wasm-bindgen Explainshell class we rely on. */
export interface WasmExplainshell {
  /** Explain a command; returns ExplainResult JSON */
  explain(command: string): string
  /** Number of manpages in the bundle */
  manpage_count(): number
  /** Release WASM-side resources */
  free(): void
}

export function isWasmGlueModule(value: unknown): value is WasmGlueModule {
  return (
    typeof value === 'object' &&
    value !== null &&
    'Explainshell' in value &&
    typeof value.Explainshell === 'function'
  )
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}

/**
 * Parse and structurally validate the ExplainResult JSON produced by
 * Explainshell.explain(). Throws on any shape mismatch.
 */
export function parseExplainResult(json: string): ExplainResult {
  const result: unknown = JSON.parse(json)
  if (!isRecord(result)) throw new Error('Unexpected response format')
  if (!('groups' in result) || !Array.isArray(result.groups)) {
    throw new Error('Unexpected response format')
  }
  for (const group of result.groups) {
    if (!isRecord(group) || typeof group.name !== 'string') {
      throw new Error('Unexpected response format')
    }
    if (!('results' in group) || !Array.isArray(group.results)) {
      throw new Error('Unexpected response format')
    }
    for (const r of group.results) {
      if (!isRecord(r) || typeof r.start !== 'number' || typeof r.end !== 'number') {
        throw new Error('Unexpected response format')
      }
    }
  }
  if ('expansions' in result && !Array.isArray(result.expansions)) {
    throw new Error('Unexpected response format')
  }
  return result as unknown as ExplainResult
}
