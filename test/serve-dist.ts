#!/usr/bin/env tsx
/**
 * Vitest globalSetup for browser tests: serves the web-target glue,
 * its _bg.wasm, and the data bundle over local HTTP, then provides
 * their URLs to tests via inject('glueUrl' | 'wasmUrl' | 'dataUrl').
 *
 * Run with: npm run test:browser
 */

import { existsSync, readFileSync } from 'node:fs'
import { type Server, createServer } from 'node:http'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import type { GlobalSetupContext } from 'vitest/node'

const __filename = fileURLToPath(import.meta.url)
const __dirname = dirname(__filename)
const distDir = resolve(__dirname, '..', 'dist')

declare module 'vitest' {
  export interface ProvidedContext {
    glueUrl: string
    wasmUrl: string
    dataUrl: string
  }
}

export default async function setup({ provide }: GlobalSetupContext) {
  const gluePath = resolve(distDir, 'wasm-web', 'explainshell.js')
  const wasmPath = resolve(distDir, 'wasm-web', 'explainshell_bg.wasm')
  const dataPath = resolve(distDir, 'explainshell.data.msgpack')
  for (const [label, p] of [
    ['glue', gluePath],
    ['wasm', wasmPath],
    ['data', dataPath],
  ] as const) {
    if (!existsSync(p)) {
      throw new Error(
        `dist ${label} is missing (${p}). Build it first: npm run build:wasm (requires Rust via rvm, see README.md)`
      )
    }
  }

  const files: Record<string, { data: Buffer; type: string }> = {
    '/wasm-web/explainshell.js': { data: readFileSync(gluePath), type: 'text/javascript' },
    '/wasm-web/explainshell_bg.wasm': { data: readFileSync(wasmPath), type: 'application/wasm' },
    '/explainshell.data.msgpack': {
      data: readFileSync(dataPath),
      type: 'application/octet-stream',
    },
  }

  const server: Server = createServer((req, res) => {
    const entry = files[req.url ?? '']
    if (entry) {
      res.writeHead(200, {
        'content-type': entry.type,
        'content-length': entry.data.length,
        'access-control-allow-origin': '*',
      })
      res.end(entry.data)
    } else {
      res.writeHead(404)
      res.end('not found')
    }
  })

  await new Promise<void>((resolvePromise) => {
    server.listen(0, '127.0.0.1', () => resolvePromise())
  })
  const address = server.address()
  if (typeof address !== 'object' || address === null) {
    throw new Error('Failed to bind test HTTP server')
  }
  const base = `http://127.0.0.1:${address.port}`
  provide('glueUrl', `${base}/wasm-web/explainshell.js`)
  provide('wasmUrl', `${base}/wasm-web/explainshell_bg.wasm`)
  provide('dataUrl', `${base}/explainshell.data.msgpack`)

  return async () => {
    await new Promise<void>((resolvePromise) => {
      server.close(() => resolvePromise())
    })
  }
}
