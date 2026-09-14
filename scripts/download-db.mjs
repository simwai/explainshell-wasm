#!/usr/bin/env node
/**
 * Download the explainshell database from GitHub releases.
 *
 * Fetches the newest `explainshell-*.db.zst` asset from the `db-latest` release,
 * verifies SHA256 against the release digest, decompresses with zstd-wasm,
 * and caches in the specified directory.
 *
 * Usage:
 *   node scripts/download-db.mjs <cache-dir> [--token <github-token>]
 *
 * Returns the path to the cached .db file on stdout.
 */

import { createHash } from 'node:crypto'
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { basename, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import zstd from '@bokuweb/zstd-wasm'
const { init, decompress } = zstd.default || zstd

await init()

const __filename = fileURLToPath(import.meta.url)
const __dirname = join(__filename, '..')

const REPO = 'idank/explainshell'
const RELEASE_TAG = 'db-latest'
const API_URL = `https://api.github.com/repos/${REPO}/releases/tags/${RELEASE_TAG}`
const ASSET_PATTERN = /^explainshell-.*\.db\.zst$/
const CACHE_MANIFEST = 'manifest.json'

function log(...args) {
  console.error('[download-db]', ...args)
}

function fail(message) {
  log('ERROR:', message)
  process.exit(1)
}

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

function loadManifest(cacheDir) {
  const path = join(cacheDir, CACHE_MANIFEST)
  if (existsSync(path)) {
    try {
      return JSON.parse(readFileSync(path, 'utf8'))
    } catch {
      return {}
    }
  }
  return {}
}

function saveManifest(cacheDir, manifest) {
  writeFileSync(join(cacheDir, CACHE_MANIFEST), JSON.stringify(manifest, null, 2))
}

async function downloadAndCacheDb(cacheDir, githubToken) {
  if (!existsSync(cacheDir)) {
    mkdirSync(cacheDir, { recursive: true })
  }

  const manifest = loadManifest(cacheDir)

  // Fetch release metadata
  log('Fetching release metadata...')
  const headers = githubToken ? { Authorization: `Bearer ${githubToken}` } : {}
  const release = await fetchJson(API_URL, headers)

  // Find newest matching asset
  const assets = release.assets
    .filter((a) => ASSET_PATTERN.test(a.name))
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

  // Check cache
  const cacheKey = expectedSha
  const cachedDbPath = join(cacheDir, `explainshell-${cacheKey}.db`)
  if (manifest[cacheKey] === cachedDbPath && existsSync(cachedDbPath)) {
    log(`Cache hit: ${cachedDbPath}`)
    return cachedDbPath
  }

  // Download
  log('Downloading...')
  const compressed = await downloadAsset(asset.browser_download_url, headers)

  // Verify SHA256
  const actualSha = sha256Hex(compressed)
  if (actualSha !== expectedSha) {
    fail(`SHA256 mismatch: expected ${expectedSha}, got ${actualSha}`)
  }
  log('SHA256 verified')

  // Decompress with zstd-wasm
  log('Decompressing...')
  const decompressed = await decompress(compressed)

  // Write cached database
  writeFileSync(cachedDbPath, decompressed)
  log(`Wrote ${(decompressed.length / 1e6).toFixed(1)} MB to ${cachedDbPath}`)

  // Update manifest
  manifest[cacheKey] = cachedDbPath
  saveManifest(cacheDir, manifest)

  return cachedDbPath
}

async function main() {
  const args = process.argv.slice(2)
  if (args.length === 0 || args.includes('--help') || args.includes('-h')) {
    console.log(`Usage: node ${basename(__filename)} <cache-dir> [--token <github-token>]`)
    console.log('')
    console.log('Downloads and caches the explainshell database from GitHub releases.')
    console.log('Outputs the path to the cached .db file on stdout.')
    process.exit(args.length === 0 ? 1 : 0)
  }

  const cacheDir = args[0]
  const tokenIndex = args.indexOf('--token')
  const githubToken = tokenIndex >= 0 && args[tokenIndex + 1] ? args[tokenIndex + 1] : null

  try {
    const dbPath = await downloadAndCacheDb(cacheDir, githubToken)
    console.log(dbPath)
  } catch (err) {
    fail(err.message)
  }
}

main()
