import * as fs from 'node:fs'
import * as path from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import type { ExplainOptions, ExplainResult, ExplainshellInstance } from '../types.js'
import { type WasmExplainshell, isWasmGlueModule, parseExplainResult } from './utils.js'

const __filename = fileURLToPath(import.meta.url)
const __dirname = path.dirname(__filename)

export class NodeExplainshell implements ExplainshellInstance {
  private inner: WasmExplainshell | null = null

  constructor(
    private gluePath: string,
    private dataPath: string
  ) {}

  async initialize(): Promise<void> {
    if (this.inner) return

    if (!fs.existsSync(this.gluePath) || !fs.existsSync(this.dataPath)) {
      throw new Error(
        `explainshell.wasm not built (missing ${this.gluePath} or ${this.dataPath}). Run: npm run build:wasm`
      )
    }

    // The experimental-nodejs-module glue loads its adjacent _bg.wasm on import.
    const mod: unknown = await import(pathToFileURL(this.gluePath).href)
    const glue = (mod as { default?: unknown }).default ?? mod
    if (!isWasmGlueModule(glue)) throw new Error('Invalid explainshell WASM glue module')

    const data = fs.readFileSync(this.dataPath)
    this.inner = new glue.Explainshell(data)
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

export interface NodeLoaderOptions {
  gluePath?: string
  dataPath?: string
  forceNew?: boolean
}

let cachedInstance: NodeExplainshell | null = null
let cachedKey: string | null = null

export async function createExplainshell(
  options?: string | NodeLoaderOptions
): Promise<ExplainshellInstance> {
  const normalized = typeof options === 'string' ? { dataPath: options } : options
  const defaultGlue = path.resolve(__dirname, '../../dist/wasm-node/explainshell.js')
  const defaultData = path.resolve(__dirname, '../../dist/explainshell.data.msgpack')
  const finalGlue = normalized?.gluePath ?? defaultGlue
  const finalData = normalized?.dataPath ?? defaultData
  const key = `${finalGlue}::${finalData}`

  if (!normalized?.forceNew && cachedInstance && cachedKey === key) return cachedInstance

  cachedInstance?.terminate()
  const instance = new NodeExplainshell(finalGlue, finalData)
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
