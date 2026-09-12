import { existsSync, readFileSync } from 'node:fs'
import { type Server, createServer } from 'node:http'
import { resolve } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import { BrowserExplainshell } from '../runtime/browser.js'
import type { ExplainshellInstance } from '../types.js'

const __filename = fileURLToPath(import.meta.url)
const __dirname = resolve(__filename, '..')
const projectRoot = resolve(__dirname, '../..')
const distDir = resolve(projectRoot, 'dist')
const gluePath = resolve(distDir, 'wasm-web', 'explainshell.js')
const wasmPath = resolve(distDir, 'wasm-web', 'explainshell_bg.wasm')
const dataPath = resolve(projectRoot, 'test', 'fixtures', 'test.data.msgpack')

const distBuilt = existsSync(gluePath) && existsSync(wasmPath) && existsSync(dataPath)

const describeIf = distBuilt ? describe : describe.skip

describeIf('Browser runtime over HTTP', () => {
  let server: Server | null = null
  let baseUrl = ''
  let explainshell: ExplainshellInstance | null = null

  beforeAll(async () => {
    const glueBytes = readFileSync(gluePath)
    const wasmBytes = readFileSync(wasmPath)
    const dataBytes = readFileSync(dataPath)

    server = createServer((req, res) => {
      if (req.url === '/wasm-web/explainshell.js') {
        res.writeHead(200, { 'content-type': 'text/javascript' })
        res.end(glueBytes)
      } else if (req.url === '/wasm-web/explainshell_bg.wasm') {
        res.writeHead(200, { 'content-type': 'application/wasm' })
        res.end(wasmBytes)
      } else if (req.url === '/explainshell.data.msgpack') {
        res.writeHead(200, { 'content-type': 'application/octet-stream' })
        res.end(dataBytes)
      } else {
        res.writeHead(404)
        res.end('not found')
      }
    })
    await new Promise<void>((resolvePromise) => {
      server?.listen(0, '127.0.0.1', () => resolvePromise())
    })
    const address = server?.address()
    if (typeof address !== 'object' || address === null) {
      throw new Error('Failed to bind test HTTP server')
    }
    baseUrl = `http://127.0.0.1:${address.port}`

    // The web-target glue is ESM: Node imports it from a file:// URL while
    // the _bg.wasm and data bundle travel over HTTP (import.meta-relative
    // fetch inside the glue would not resolve file://).
    const instance = new BrowserExplainshell(
      pathToFileURL(gluePath).href,
      `${baseUrl}/wasm-web/explainshell_bg.wasm`,
      `${baseUrl}/explainshell.data.msgpack`
    )
    await instance.initialize()
    explainshell = instance
  }, 60000)

  afterAll(async () => {
    explainshell?.terminate()
    explainshell = null
    await new Promise<void>((resolvePromise) => {
      if (server) server.close(() => resolvePromise())
      else resolvePromise()
    })
    server = null
  })

  const getExplainshell = (): ExplainshellInstance => {
    if (!explainshell) throw new Error('explainshell not initialized')
    return explainshell
  }

  it('serves the wasm with the correct MIME type', async () => {
    const res = await fetch(`${baseUrl}/wasm-web/explainshell_bg.wasm`)
    expect(res.ok).toBe(true)
    expect(res.headers.get('content-type')).toBe('application/wasm')
  })

  it('explains through the browser runtime', async () => {
    const result = await getExplainshell().explain('tar -xvf archive.tar')
    expect(result.groups.length).toBeGreaterThan(0)
  })

  it('exposes the fixture bundle size', () => {
    expect(getExplainshell().manpageCount()).toBe(4)
  })

  it('imports the glue from a file URL for reference', () => {
    expect(pathToFileURL(gluePath).href).toContain('explainshell.js')
  })
})
