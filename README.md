# explainshell-wasm

explainshell compiled to WebAssembly, with a typed TypeScript API for Node.js and browsers.

The module is built with Rust (`wasm32-unknown-unknown`) via wasm-bindgen, exposing the `Explainshell` class (`new(data)`, `explain(command)`, `manpage_count()`). The manpage corpus is exported from explainshell's SQLite database to a MessagePack bundle at build time; both runtimes load the same artifacts.

## Requirements

- Node.js >= 20
- Rust via [rvm-windows](https://github.com/MemoryClear/rvm-windows) (`rvm use stable`), with the `wasm32-unknown-unknown` target
- wasm-bindgen-cli matching the `wasm-bindgen` crate version (see Cargo.lock)
- Python 3 + `msgpack` (data export only)
- The prebuilt `dist/` artifacts (see [Building](#building)), or build them yourself

## Installation

```bash
npm install explainshell-wasm
```

## Data notice

Explaining commands requires the manpage database. **Since v0.1.1, the build automatically downloads the full database from explainshell's GitHub releases** (the `db-latest` tag, ~20-30 MB MessagePack bundle covering thousands of manpages from Ubuntu/Arch). No manual setup required.

Data source priority (first match wins):

1. `$EXPLAINSHELL_DB` environment variable — explicit path to a local `explainshell.db`
2. `./explainshell.db` in the repo root — local database file
3. `.explainshell-cache/` — cached auto-downloaded database (gitignored)
4. **Auto-download from GitHub releases** — fetches newest `explainshell-*.db.zst`, verifies SHA256, decompresses with zstd-wasm
5. CI fixture fallback (`test/fixtures/test.db`) — 4 manpages only, with loud warning

To force the CI fixture (e.g., for minimal test builds):
```bash
EXPLAINSHELL_DB= npm run build  # empty value skips auto-download
```

**License note:** The database contains upstream manpage text (GPL, BSD, MIT, etc.). See `LICENSE-DATABASE.md` for redistribution terms. The `source` field in each entry identifies the originating package.

## Publishing

One-step release:

```bash
npm run release-patch   # 0.1.0 -> 0.1.1
npm run release-minor   # 0.1.0 -> 0.2.0
npm run release-major   # 0.1.0 -> 1.0.0
```

This runs tests, bumps the version, rebuilds `dist/`, pushes the tag, and publishes to npm.

For manual control:

```bash
npm run build
npm publish
```

## Usage

### Node.js

```typescript
import { createExplainshell, explain } from 'explainshell-wasm'

// One-shot
const result = await explain('tar -xvf archive.tar')

// Reusable instance (WASM module is instantiated once and reused)
const explainshell = await createExplainshell()
console.log('manpages:', explainshell.manpageCount())
const result2 = await explainshell.explain('sudo tar -xvf archive.tar')
```

By default the loader resolves `dist/wasm-node/explainshell.js` and
`dist/explainshell.data.msgpack` relative to the package. Pass explicit paths when needed:

```typescript
const explainshell = await createExplainshell({
  dataPath: '/path/to/explainshell.data.msgpack',
})
```

### Browser

Serve `dist/wasm-web/` and `dist/explainshell.data.msgpack` from the same origin with correct MIME types (`application/wasm` for `.wasm`, `text/javascript` for `.js`, `application/octet-stream` for `.msgpack`), then:

```typescript
import { createExplainshell } from 'explainshell-wasm'

const explainshell = await createExplainshell({
  runtime: 'browser',
  glueUrl: '/wasm-web/explainshell.js',
  wasmUrl: '/wasm-web/explainshell_bg.wasm',
  dataUrl: '/explainshell.data.msgpack',
})
const result = await explainshell.explain(editorValue)
console.log(result)
```

## API reference

### `createExplainshell(options?)`

Creates (or returns the cached) explainshell instance, instantiating the WASM module on first call. Pass `forceNew: true` to bypass the cache.

```typescript
await createExplainshell(options?: {
  gluePath?: string;             // node: path of the wasm-bindgen glue
  dataPath?: string;             // node: path of the .data.msgpack bundle
  glueUrl?: string;              // browser: URL of the wasm-bindgen glue
  wasmUrl?: string;              // browser: URL of the _bg.wasm module
  dataUrl?: string;              // browser: URL of the .data.msgpack bundle
  runtime?: 'node' | 'browser' | 'auto';  // default 'auto'
  forceNew?: boolean;            // bypass cache (default false)
});
```

### `explain(command, options?)`

```typescript
await explain('git commit -m "msg"') // ExplainResult
```

### `resetExplainshell()`

Terminates the cached instance. Mainly useful in tests.

### `ExplainOptions`

```typescript
interface ExplainOptions {
  distro?: string // reserved for multi-distro bundles
  release?: string // reserved for multi-distro bundles
}
```

### `ExplainResult`

```typescript
interface ExplainResult {
  groups: MatchGroup[] // one "shell" group plus one per command
  expansions: Expansion[]
}

interface MatchGroup {
  name: string // e.g. "shell", "command1"
  results: MatchResult[]
  manpage?: ParsedManpage | null
  suggestions?: ParsedManpage[]
  error?: string | null
  positional_index: number
}

interface MatchResult {
  start: number
  end: number
  text?: string | null // help text, null when unknown
  match_text?: string | null // matched input span
  debug_info?: Record<string, unknown> | null
}
```

## How it works

```
explainshell.db (SQLite, built by the explainshell pipeline)
  │  scripts/export_wasm_data.py (msgpack)
  ▼
dist/explainshell.data.msgpack  (versioned bundle: manpages + mappings)
  │  crates/{core,data,parse,match} (Rust)
  │  cargo build --target wasm32-unknown-unknown
  │  wasm-bindgen --target {nodejs,web}
  ▼
dist/wasm-{node,web}/explainshell.js (+ _bg.wasm)
  │  src/runtime/{node,browser}.ts
  ▼
  explain(command) → Promise<ExplainResult>
```

Key design points:

- **Single entry point.** `Explainshell.explain()` takes a command string and returns `ExplainResult` JSON. The shell parser (bashlex port) and matcher run entirely inside the module.
- **In-memory data.** Manpage lookup resolves against the bundled MessagePack data; there is no host filesystem or network access from WASM. Repeated calls share no mutable state.
- **No WASI.** The module is pure computation on `wasm32-unknown-unknown`, so both runtimes use the stock wasm-bindgen glue with no WASI shim.
- **Cache discipline.** `createExplainshell` caches by artifact path; `forceNew: true` bypasses it.

## Building

Requires Rust (see Requirements). Python is needed only for the data export step.

```bash
# Toolchain setup (Windows, one time)
# install https://github.com/MemoryClear/rvm-windows, then:
rvm use stable
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.128 --locked
pip install msgpack

npm run build:wasm   # Rust -> wasm-bindgen -> data bundle, all in dist/
npm run build        # build:wasm + TypeScript (via unbuild)
```

`scripts/build-wasm.mjs` is cross-platform (Windows/Linux/macOS). The
`wasm32-wasi` target name from older docs is now `wasm32-wasip1` in Rust;
this project does not use it (see "No WASI" above).

**Auto-download:** On first build (or when the upstream `db-latest` release updates), the script downloads ~100 MB compressed database, verifies SHA256, decompresses to `.explainshell-cache/`, and exports the MessagePack bundle. Subsequent builds use the cache. Set `GITHUB_TOKEN` env var for higher API rate limits (60/hr unauthenticated).

## Development

```bash
npm install          # install JS dependencies
npm run build        # builds WASM + TypeScript (via unbuild)
npm test             # Node suites: unit + integration over HTTP
npm run test:browser # real headless Chromium via @vitest/browser + Playwright
npm run lint         # Biome check
npm run lint:fix     # Biome check --write
```

Git hooks (Husky + lint-staged) run Biome on staged JS/TS files at commit and the test suite on push.

### Test matrix

| Suite                   | Environment            | What it covers                                          |
| ----------------------- | ---------------------- | ------------------------------------------------------- |
| `api.test.ts`           | Node                   | Type shapes, response envelopes                         |
| `matcher.test.ts`       | Node + built `.wasm`   | explain() groups, unknown-program errors, empty input   |
| `browser-serve.test.ts` | Node + local HTTP      | `BrowserExplainshell` fetching glue/wasm/data over HTTP |
| `browser.test.ts`       | Real headless Chromium | Full browser path: fetch, instantiate, explain          |

Integration suites skip automatically when `dist/` is not built.

### Project structure

```
explainshell-wasm/
├── Cargo.toml                  # Rust workspace
├── crates/
│   ├── core/                   # domain types (CliOption, ParsedManpage, ...)
│   ├── data/                   # msgpack bundle loading + lookup
│   ├── parse/                  # shell parser (bashlex port)
│   ├── match/                  # matcher algorithm
│   └── wasm/                   # wasm-bindgen exports (Explainshell)
├── src/
│   ├── index.ts                # public entry point
│   ├── api.ts                  # createExplainshell / explain / resetExplainshell
│   ├── types.ts                # ExplainOptions, ExplainResult, ...
│   ├── runtime/
│   │   ├── node.ts             # Node loader (reads dist/ from disk)
│   │   ├── browser.ts          # browser loader (fetch)
│   │   └── utils.ts            # glue shape guards, response validation
│   └── __tests__/              # Vitest suites
├── test/
│   ├── fixtures/               # test.db + test.data.msgpack (CI fixture)
│   └── serve-dist.ts           # globalSetup: serves dist/ to browser tests
├── scripts/                    # build-wasm.mjs, export_wasm_data.py, ...
├── build.config.ts             # unbuild configuration
├── vitest.config.ts            # Node suites
└── vitest.browser.config.ts    # Chromium suite
```

`dist/` (compiled JS, `.wasm`, glue, data bundle, generated `.d.ts`) and `target/` are gitignored build outputs.

## Known limitations

- The default bundle is the 4-manpage CI fixture; real coverage needs a full `explainshell.db` (see Data notice).
- The data bundle for a full distro is ~20-30 MB; reuse the instance returned by `createExplainshell`.
- The matcher currently resolves commands to manpages; full token-level option matching lands with the bashlex port.
- Browser testing covers headless Chromium. Other engines should work (the module only needs post-MVP features all modern browsers ship), but they are not in the matrix.