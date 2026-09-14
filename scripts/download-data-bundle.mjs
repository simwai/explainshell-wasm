#!/usr/bin/env node
/**
 * Download the explainshell data bundle (explainshell.data.msgpack) from GitHub releases.
 *
 * This runs at npm install time (postinstall) so users don't need the full toolchain.
 * The bundle is ~250 MB and contains 61322 manpages.
 *
 * Usage:
 *   node scripts/download-data-bundle.mjs [--token <github-token>]
 *
 * Downloads to dist/explainshell.data.msgpack
 */

import { createHash } from 'node:crypto'
import { existsSync, mkdirSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import zstd from '@bokuweb/zstd-wasm'

const __filename = fileURLToPath(import.meta.url)
const __dirname = dirname(__filename)
const projectRoot = resolve(__dirname, '..')
const DIST_DIR = join(projectRoot, 'dist')
const DATA_BUNDLE_PATH = join(DIST_DIR, 'explainshell.data.msgpack')

const REPO = 'idank/explainshell'
const RELEASE_TAG = 'db-latest'
const API_URL = `https://api.github.com/repos/${REPO}/releases/tags/${RELEASE_TAG}`
const ASSET_PATTERN = /^explainshell-.*\.db\.zst$/

function log(...args) {
  console.error('[download-data-bundle]', ...args)
}

function fail(message) {
  log('ERROR:', message)
  process.exit(1)
}

async function fetchJson(url, headers = {}) {
  const res = await fetch(url, {
    headers: {
      'Accept': 'application/vnd.github+json',
      'User-Agent': 'explainshell-wasm-postinstall',
      ...headers
    }
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

async function downloadAndExportDataBundle(githubToken) {
  if (!existsSync(DIST_DIR)) {
    mkdirSync(DIST_DIR, { recursive: true })
  }

  // Check if already exists
  if (existsSync(DATA_BUNDLE_PATH)) {
    log('Data bundle already exists, skipping download')
    return
  }

  log('Fetching release metadata...')
  const headers = githubToken ? { Authorization: `Bearer ${githubToken}` } : {}
  const release = await fetchJson(API_URL, headers)

  // Find newest matching asset
  const assets = release.assets
    .filter(a => ASSET_PATTERN.test(a.name))
    .sort((a, b) => new Date(b.created_at) - new Date(a.created_at))

  if (assets.length === 0) {
    fail('No matching explainshell-*.db.zst assets found in db-latest release')
  }

  const asset = assets[0]
  const expectedSha = asset.digest?.replace('sha256:', '')
  if (!expectedSha) {
    fail('Asset missing SHA256 digest in release metadata')
  }

  log(`Selected asset: ${asset.name} (${(asset.size / 1e6).toFixed(1)} MB)`)

  // Download
  log('Downloading database...')
  const compressed = await downloadAsset(asset.browser_download_url, headers)

  // Verify SHA256
  const actualSha = sha256Hex(compressed)
  if (actualSha !== expectedSha) {
    fail(`SHA256 mismatch: expected ${expectedSha}, got ${actualSha}`)
  }
  log('SHA256 verified')

  // Decompress with zstd-wasm
  log('Decompressing...')
  const { init, decompress } = zstd.default || zstd
  await init()
  const decompressed = await decompress(compressed)

  log('Exporting to MessagePack...')
  const { spawnSync } = await import('node:child_process')
  const python = (await import('node:child_process')).spawnSync('python3', ['--version'], { stdio: 'ignore' }).status === 0 ? 'python3' : 'python'
  
  // Write temporary database file
  const tempDbPath = join(DIST_DIR, `temp-explainshell-${Date.now()}.db`)
  writeFileSync(tempDbPath, decompressed)
  log(`Wrote temporary database: ${(decompressed.length / 1e6).toFixed(1)} MB`)

  // Run export script
  const exportResult = spawnSync(python, [
    'scripts/export_wasm_data.py',
    tempDbPath,
    DATA_BUNDLE_PATH
  ], { stdio: 'inherit', cwd: projectRoot })

  // Clean up temp file
  try { require('node:fs').unlinkSync(tempDbPath) } catch {}

  if (exportResult.status !== 0) {
    fail('Failed to export data bundle')
  }

  log(`Data bundle written to ${DATA_BUNDLE_PATH}`)
}

async function main() {
  const args = process.argv.slice(2)
  if (args.includes('--help') || args.includes('-h')) {
    console.log(`Usage: node ${__filename} [--token <github-token>]`)
    console.log('')
    console.log('Downloads the explainshell data bundle from GitHub releases.')
    console.log('Outputs to dist/explainshell.data.msgpack')
    process.exit(0)
  }

  const tokenIndex = args.indexOf('--token')
  const githubToken = tokenIndex >= 0 && args[tokenIndex + 1] ? args[tokenIndex + 1] : null

  try {
    await downloadAndExportDataBundle(githubToken)
  } catch (err) {
    fail(err.message)
  }
}

main()