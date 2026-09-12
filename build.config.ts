import { defineBuildConfig } from 'unbuild'
import { existsSync, mkdirSync } from 'node:fs'
import { resolve } from 'node:path'

export default defineBuildConfig({
  entries: [
    'src/index.ts',
    'src/runtime/node.ts',
    'src/runtime/browser.ts',
  ],
  outDir: 'dist',
  declaration: 'compatible',
  externals: [],
  clean: false,
  failOnWarn: false,
  hooks: {
    async 'build:done'() {
      // The WASM glue and data bundle are produced by build:wasm.mjs
      // (wasm-bindgen writes straight into dist/). Verify they exist so a
      // TypeScript-only build never ships a package without its engine.
      const projectRoot = process.cwd()
      const distDir = resolve(projectRoot, 'dist')
      mkdirSync(distDir, { recursive: true })

      const required = [
        'wasm-node/explainshell.js',
        'wasm-node/explainshell_bg.wasm',
        'wasm-web/explainshell.js',
        'wasm-web/explainshell_bg.wasm',
        'explainshell.data.msgpack',
      ]
      const missing = required.filter((f) => !existsSync(resolve(distDir, f)))
      if (missing.length > 0) {
        console.warn(`⚠️  missing build artifacts in dist/: ${missing.join(', ')} (run build:wasm first)`)
        return
      }
      console.log('✅ dist/ contains WASM glue (node + web) and data bundle')
    },
  },
})
