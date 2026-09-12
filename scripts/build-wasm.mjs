#!/usr/bin/env node
/**
 * Build orchestrator for explainshell-wasm (cross-platform).
 *
 * Rust -> wasm32-unknown-unknown -> wasm-bindgen (nodejs + web targets),
 * then export the manpage data bundle to MessagePack.
 *
 * Requires: cargo (via rvm: `rvm use stable`), wasm-bindgen-cli matching the
 * wasm-bindgen crate version (see Cargo.lock), python + msgpack, node.
 *
 *   cargo install wasm-bindgen-cli --version 0.2.128 --locked
 *   pip install msgpack
 *
 * Data source (first match wins):
 *   $EXPLAINSHELL_DB, ./explainshell.db, else the CI fixture test.db
 *   (with a loud warning).
 */

import { spawnSync } from 'node:child_process'
import { existsSync, mkdirSync, statSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const __filename = fileURLToPath(import.meta.url)
const __dirname = dirname(__filename)
const projectRoot = resolve(__dirname, '..')

function fail(message) {
  console.error(`ERROR: ${message}`)
  process.exit(1)
}

function run(cmd, args, options = {}) {
  const result = spawnSync(cmd, args, { stdio: 'inherit', cwd: projectRoot, ...options })
  if (result.error?.code === 'ENOENT') return false
  if (result.status !== 0) fail(`command failed: ${cmd} ${args.join(' ')}`)
  return true
}

function have(cmd) {
  const probe = process.platform === 'win32' ? 'where' : 'command'
  const probeArgs = process.platform === 'win32' ? [cmd] : ['-v', cmd]
  const result = spawnSync(probe, probeArgs, { stdio: 'ignore' })
  return result.status === 0
}

// rvm keeps cargo outside PATH until `rvm use` runs in a fresh terminal.
// Pick it up automatically when present so the build just works.
if (!have('cargo') && process.platform === 'win32' && process.env.USERPROFILE) {
  const rvmCargo = join(process.env.USERPROFILE, '.rvm', 'cargo', 'bin')
  if (existsSync(join(rvmCargo, 'cargo.exe'))) {
    process.env.PATH = `${rvmCargo};${process.env.PATH}`
    process.env.RUSTUP_HOME ??= join(process.env.USERPROFILE, '.rvm', 'rustup')
    process.env.CARGO_HOME ??= join(process.env.USERPROFILE, '.rvm', 'cargo')
    console.log(`Using rvm cargo at ${rvmCargo}`)
  }
}

if (!have('cargo')) fail('cargo not found in PATH (try: rvm use stable)')
if (!have('wasm-bindgen')) {
  fail(
    'wasm-bindgen not found in PATH (install: cargo install wasm-bindgen-cli --version 0.2.128 --locked)'
  )
}

let python = null
for (const candidate of ['python3', 'python']) {
  // Probe with --version: on Windows `python3` can be a dead Microsoft Store
  // stub that exists on PATH but fails to run.
  const probe = spawnSync(candidate, ['--version'], { stdio: 'ignore' })
  if (!probe.error && probe.status === 0) {
    python = candidate
    break
  }
}
if (!python) fail('python not found in PATH')

console.log('Building explainshell-wasm...')
run('cargo', [
  'build',
  '--release',
  '--target',
  'wasm32-unknown-unknown',
  '-p',
  'explainshell-wasm',
])

const wasmPath = join(
  projectRoot,
  'target',
  'wasm32-unknown-unknown',
  'release',
  'explainshell_wasm.wasm'
)
if (!existsSync(wasmPath)) fail(`built wasm module not found at ${wasmPath}`)

console.log('Running wasm-bindgen (nodejs + web targets)...')
mkdirSync(join(projectRoot, 'dist', 'wasm-node'), { recursive: true })
mkdirSync(join(projectRoot, 'dist', 'wasm-web'), { recursive: true })
run('wasm-bindgen', [
  '--target',
  'experimental-nodejs-module',
  '--out-dir',
  'dist/wasm-node',
  '--typescript',
  wasmPath,
  '--out-name',
  'explainshell',
])
run('wasm-bindgen', [
  '--target',
  'web',
  '--out-dir',
  'dist/wasm-web',
  '--typescript',
  wasmPath,
  '--out-name',
  'explainshell',
])

console.log('Exporting manpage data bundle...')
let db = process.env.EXPLAINSHELL_DB || null
if (!db && existsSync(join(projectRoot, 'explainshell.db')))
  db = join(projectRoot, 'explainshell.db')
if (!db) {
  console.warn('WARNING: no explainshell.db found -- bundling the CI fixture test.db instead.')
  console.warn('WARNING: set EXPLAINSHELL_DB or place explainshell.db here for full data.')
  run(python, ['scripts/make_test_db.py', 'test/fixtures/test.db'])
  db = join(projectRoot, 'test', 'fixtures', 'test.db')
}
run(python, ['scripts/export_wasm_data.py', db, 'dist/explainshell.data.msgpack'])

const wasmSize = statSync(join(projectRoot, 'dist', 'wasm-node', 'explainshell_bg.wasm')).size
const dataSize = statSync(join(projectRoot, 'dist', 'explainshell.data.msgpack')).size

// Marker so loaders and tests can detect a complete build.
writeFileSync(
  join(projectRoot, 'dist', '.build-complete'),
  `explainshell-wasm ${new Date().toISOString()}\n`
)

console.log('')
console.log('Build complete!')
console.log(`  dist/wasm-node/explainshell.js (+ _bg.wasm, ${wasmSize} bytes)`)
console.log('  dist/wasm-web/explainshell.js  (+ _bg.wasm)')
console.log(`  dist/explainshell.data.msgpack (${dataSize} bytes)`)
console.log('')
console.log('Test with: npm test')
