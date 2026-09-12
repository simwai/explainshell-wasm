import { existsSync } from 'node:fs'
import { resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { afterAll, describe, expect, it } from 'vitest'
import { NodeExplainshell } from '../runtime/node.js'
import type { ExplainshellInstance } from '../types.js'

const __filename = fileURLToPath(import.meta.url)
const __dirname = resolve(__filename, '..')
const projectRoot = resolve(__dirname, '../..')
const gluePath = resolve(projectRoot, 'dist', 'wasm-node', 'explainshell.js')
const dataPath = resolve(projectRoot, 'test', 'fixtures', 'test.data.msgpack')

const engineBuilt = existsSync(gluePath) && existsSync(dataPath)

const describeIf = engineBuilt ? describe : describe.skip

describeIf('Matcher through the Node runtime (fixture bundle)', () => {
  let explainshell: ExplainshellInstance | null = null

  afterAll(() => {
    explainshell?.terminate()
    explainshell = null
  })

  it('explains a known command against its manpage', async () => {
    const instance = new NodeExplainshell(gluePath, dataPath)
    await instance.initialize()
    explainshell = instance

    expect(instance.manpageCount()).toBe(4)
    const result = await instance.explain('tar -xvf archive.tar')
    expect(Array.isArray(result.groups)).toBe(true)
    expect(result.groups.length).toBeGreaterThan(0)
  })

  it('rejects unknown programs with a typed error', async () => {
    const instance = new NodeExplainshell(gluePath, dataPath)
    await instance.initialize()
    explainshell?.terminate()
    explainshell = instance

    await expect(instance.explain('definitelynotarealcmd-xyz foo')).rejects.toThrow(
      /unknown program/
    )
  })

  it('rejects empty input', async () => {
    const instance = new NodeExplainshell(gluePath, dataPath)
    await instance.initialize()
    explainshell?.terminate()
    explainshell = instance

    await expect(instance.explain('   ')).rejects.toThrow()
  })
})
