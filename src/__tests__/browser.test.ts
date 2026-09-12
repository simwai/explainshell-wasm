import { afterAll, beforeAll, describe, expect, inject, it } from 'vitest'
import type { ExplainshellInstance } from '../types.js'

const isBrowser = typeof window !== 'undefined' && typeof document !== 'undefined'

const describeIf = isBrowser ? describe : describe.skip

function injected(name: 'glueUrl' | 'wasmUrl' | 'dataUrl'): string {
  const value: unknown = inject(name)
  if (typeof value !== 'string' || value.length === 0) {
    throw new Error(`${name} not provided`)
  }
  return value
}

describeIf('Browser Integration Tests', () => {
  let explainshell: ExplainshellInstance | null = null

  beforeAll(async () => {
    if (!isBrowser) {
      console.warn('Not in browser environment, skipping browser tests')
      return
    }

    const { createExplainshell } = await import('../runtime/browser.js')
    explainshell = await createExplainshell({
      glueUrl: injected('glueUrl'),
      wasmUrl: injected('wasmUrl'),
      dataUrl: injected('dataUrl'),
    })
  }, 120000)

  const getExplainshell = (): ExplainshellInstance => {
    if (!explainshell) throw new Error('explainshell not initialized')
    return explainshell
  }

  afterAll(() => {
    explainshell?.terminate()
    explainshell = null
  })

  it('loads the fixture bundle in the browser', async () => {
    expect(getExplainshell().manpageCount()).toBe(4)
  })

  it('explains a known command', async () => {
    const result = await getExplainshell().explain('tar -xvf archive.tar')

    expect(Array.isArray(result.groups)).toBe(true)
    expect(result.groups.length).toBeGreaterThan(0)
  })

  it('rejects unknown programs', async () => {
    await expect(getExplainshell().explain('definitelynotarealcmd-xyz foo')).rejects.toThrow(
      /unknown program/
    )
  })
})

describe('Browser API Shape (Mock)', () => {
  it('should have correct createExplainshell signature', async () => {
    type Expected = (
      options?: string | { dataUrl?: string; forceNew?: boolean }
    ) => Promise<import('../types.js').ExplainshellInstance>

    const { createExplainshell } = await import('../runtime/browser.js')
    const _check: Expected = createExplainshell
    expect(_check).toBeDefined()
  })
})
