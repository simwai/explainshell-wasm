import type { ExplainOptions, ExplainResult, ExplainshellInstance } from '../types.js'
import { type WasmExplainshell, isWasmGlueModule, parseExplainResult } from './utils.js'

export class BrowserExplainshell implements ExplainshellInstance {
  private inner: WasmExplainshell | null = null

  constructor(
    private glueUrl: string,
    private wasmUrl: string,
    private dataUrl: string
  ) {}

  async initialize(): Promise<void> {
    if (this.inner) return

    const mod: unknown = await import(/* @vite-ignore */ this.glueUrl)
    if (!isWasmGlueModule(mod)) throw new Error('Invalid explainshell WASM glue module')
    if (typeof mod.default === 'function') {
      await mod.default(this.wasmUrl)
    }

    const response = await fetch(this.dataUrl)
    if (!response.ok) {
      throw new Error(`Failed to fetch data bundle: ${response.status} ${response.statusText}`)
    }
    const data = new Uint8Array(await response.arrayBuffer())
    this.inner = new mod.Explainshell(data)
  }

  private requireInner(): WasmExplainshell {
    if (!this.inner) throw new Error('Not initialized')
    return this.inner
  }

  async explain(command: string, _options?: ExplainOptions): Promise<ExplainResult> {
    if (!this.inner) await this.initialize()
    void _options
    return parseExplainResult(this.requireInner().explain(command))
  }

  async explainAst(astJson: string, _options?: ExplainOptions): Promise<ExplainResult> {
    if (!this.inner) await this.initialize()
    void _options
    return parseExplainResult(this.requireInner().explain_ast(astJson))
  }

  manpageCount(): number {
    return this.requireInner().manpage_count()
  }

  terminate(): void {
    this.inner?.free()
    this.inner = null
  }
}

export interface BrowserLoaderOptions {
  glueUrl?: string
  wasmUrl?: string
  dataUrl?: string
  forceNew?: boolean
}

let cachedInstance: BrowserExplainshell | null = null
let cachedKey: string | null = null

export async function createExplainshell(
  options?: string | BrowserLoaderOptions
): Promise<ExplainshellInstance> {
  const normalized = typeof options === 'string' ? { dataUrl: options } : options
  const finalGlue = normalized?.glueUrl ?? '/wasm-web/explainshell.js'
  const finalWasm = normalized?.wasmUrl ?? '/wasm-web/explainshell_bg.wasm'
  const finalData = normalized?.dataUrl ?? '/explainshell.data.msgpack'
  const key = `${finalGlue}::${finalWasm}::${finalData}`

  if (!normalized?.forceNew && cachedInstance && cachedKey === key) return cachedInstance

  cachedInstance?.terminate()
  const instance = new BrowserExplainshell(finalGlue, finalWasm, finalData)
  await instance.initialize()
  cachedInstance = instance
  cachedKey = key
  return instance
}

export function resetCache(): void {
  cachedInstance?.terminate()
  cachedInstance = null
  cachedKey = null
}
