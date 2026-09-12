import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { defineConfig } from 'vitest/config'

const __filename = fileURLToPath(import.meta.url)
const __dirname = dirname(__filename)

export default defineConfig({
  test: {
    globals: true,
    include: ['src/__tests__/browser.test.ts'],
    testTimeout: 120000,
    hookTimeout: 120000,
    globalSetup: [resolve(__dirname, './test/serve-dist.ts')],
    browser: {
      enabled: true,
      provider: 'playwright',
      name: 'chromium',
    },
  },
  resolve: {
    alias: {
      '@': resolve(__dirname, './src'),
    },
  },
})
