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
 *   $EXPLAINSHELL_DB, ./explainshell.db, .explainshell-cache/, auto-download from GitHub releases,
 *   else the CI fixture test.db (with a loud warning).
 */

import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
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

// --- Database auto-download ---
const REPO = 'idank/explainshell'
const RELEASE_TAG = 'db-latest'
const API_URL = `https://api.github.com/repos/${REPO}/releases/tags/${RELEASE_TAG}`
const ASSET_PATTERN = /^explainshell-.*\.db\.zst$/

async function fetchJson(url, headers = {}) {
  const res = await fetch(url, {
    headers: {
      Accept: 'application/vnd.github+json',
      'User-Agent': 'explainshell-wasm-build',
      ...headers,
    },
  })
  if (!res.ok) {
    const text = await res.text()
    fail(`HTTP ${res.status} from ${url}: ${text}`)
  }
  return res.json()
}

async function downloadAsset(url, headers = {}) {
  const res = await fetch(url, { headers })
  if (!res.ok) {
    const text = await res.text()
    fail(`HTTP ${res.status} downloading asset: ${text}`)
  }
  return new Uint8Array(await res.arrayBuffer())
}

function sha256Hex(data) {
  return createHash('sha256').update(data).digest('hex')
}

async function downloadAndCacheDb(cacheDir, githubToken) {
  const {
    existsSync: exists,
    mkdirSync: mkd,
    readFileSync,
    writeFileSync,
  } = await import('node:fs')
  const { join: j } = await import('node:path')
  const zstd = await import('@bokuweb/zstd-wasm')
  const { init, decompress } = zstd.default || zstd
  await init()

  if (!exists(cacheDir)) mkd(cacheDir, { recursive: true })

  const MANIFEST = 'manifest.json'
  function loadManifest() {
    const path = j(cacheDir, MANIFEST)
    if (exists(path)) {
      try {
        return JSON.parse(readFileSync(path, 'utf8'))
      } catch {
        return {}
      }
    }
    return {}
  }
  function saveManifest(m) {
    writeFileSync(j(cacheDir, MANIFEST), JSON.stringify(m, null, 2))
  }

  const manifest = loadManifest()

  // Fetch release metadata
  console.log('Fetching release metadata...')
  const headers = githubToken ? { Authorization: `Bearer ${githubToken}` } : {}
  const release = await fetchJson(API_URL, headers)

  // Find newest matching asset
  const assets = release.assets
    .filter((a) => ASSET_PATTERN.test(a.name))
    .sort((a, b) => new Date(b.created_at) - new Date(a.created_at))

  if (assets.length === 0) {
    throw new Error('No matching explainshell-*.db.zst assets found in db-latest release')
  }

  const asset = assets[0]
  const expectedSha = asset.digest?.replace('sha256:', '')
  if (!expectedSha) {
    throw new Error('Asset missing SHA256 digest in release metadata')
  }

  console.log(`Selected asset: ${asset.name} (${(asset.size / 1e6).toFixed(1)} MB)`)

  // Check cache
  const cacheKey = expectedSha
  const cachedDbPath = j(cacheDir, `explainshell-${cacheKey}.db`)
  if (manifest[cacheKey] === cachedDbPath && exists(cachedDbPath)) {
    console.log(`Cache hit: ${cachedDbPath}`)
    return cachedDbPath
  }

  // Download
  console.log('Downloading database...')
  const compressed = await downloadAsset(asset.browser_download_url, headers)

  // Verify SHA256
  const actualSha = sha256Hex(compressed)
  if (actualSha !== expectedSha) {
    throw new Error(`SHA256 mismatch: expected ${expectedSha}, got ${actualSha}`)
  }
  console.log('SHA256 verified')

  // Decompress with zstd-wasm
  console.log('Decompressing...')
  const decompressed = await decompress(compressed)

  // Write cached database
  writeFileSync(cachedDbPath, decompressed)
  console.log(`Wrote ${(decompressed.length / 1e6).toFixed(1)} MB to ${cachedDbPath}`)

  // Update manifest
  manifest[cacheKey] = cachedDbPath
  saveManifest(manifest)

  return cachedDbPath
}

async function main() {
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
  if (!db && existsSync(join(projectRoot, 'explainshell.db'))) {
    db = join(projectRoot, 'explainshell.db')
  }
  if (!db) {
    // Try auto-download from GitHub releases
    const cacheDir = join(projectRoot, '.explainshell-cache')
    const githubToken = process.env.GITHUB_TOKEN || null
    try {
      db = await downloadAndCacheDb(cacheDir, githubToken)
      console.log(`Using auto-downloaded database: ${db}`)
    } catch (err) {
      console.warn('WARNING: auto-download failed:', err.message)
      console.warn('WARNING: falling back to CI fixture test.db (4 manpages only).')
      console.warn('WARNING: set EXPLAINSHELL_DB or place explainshell.db here for full data.')
      run(python, ['scripts/make_test_db.py', 'test/fixtures/test.db'])
      db = join(projectRoot, 'test', 'fixtures', 'test.db')
    }
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
}

main().catch((err) => {
  console.error('ERROR:', err.message)
  process.exit(1)
})
