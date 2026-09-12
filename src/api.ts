import type { ExplainOptions, ExplainResult, ExplainshellInstance } from './types.js'

let cachedInstance: ExplainshellInstance | null = null

export interface CreateExplainshellOptions {
  gluePath?: string
  dataPath?: string
  glueUrl?: string
  wasmUrl?: string
  dataUrl?: string
  runtime?: 'node' | 'browser' | 'auto'
  forceNew?: boolean
}

export async function createExplainshell(
  options?: CreateExplainshellOptions
): Promise<ExplainshellInstance> {
  if (cachedInstance && !options?.forceNew) return cachedInstance

  const isNode = typeof process !== 'undefined' && process.versions?.node
  const isBrowser = typeof window !== 'undefined' && typeof document !== 'undefined'

  let runtime = options?.runtime || 'auto'
  if (runtime === 'auto') {
    runtime = isNode ? 'node' : isBrowser ? 'browser' : 'node'
  }

  let instance: ExplainshellInstance

  if (runtime === 'node') {
    const { createExplainshell: createNodeExplainshell } = await import('./runtime/node.js')
    instance = await createNodeExplainshell({
      gluePath: options?.gluePath,
      dataPath: options?.dataPath,
      forceNew: options?.forceNew,
    })
  } else if (runtime === 'browser') {
    const { createExplainshell: createBrowserExplainshell } = await import('./runtime/browser.js')
    instance = await createBrowserExplainshell({
      glueUrl: options?.glueUrl,
      wasmUrl: options?.wasmUrl,
      dataUrl: options?.dataUrl,
      forceNew: options?.forceNew,
    })
  } else {
    throw new Error(`Unknown runtime: ${runtime}`)
  }

  cachedInstance = instance
  return instance
}

/** One-shot explain using the cached instance. */
export async function explain(command: string, options?: ExplainOptions): Promise<ExplainResult> {
  const explainshell = await createExplainshell()
  return explainshell.explain(command, options)
}

export function resetExplainshell(): void {
  cachedInstance?.terminate()
  cachedInstance = null
}

export type {
  CliOption,
  ExplainOptions,
  ExplainResult,
  ExplainResultGroup,
  Expansion,
  ExplainshellInstance,
  MatchGroup,
  MatchResult,
  ParsedManpage,
} from './types.js'
