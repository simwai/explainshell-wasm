/**
 * Vitest ProvidedContext augmentation for browser tests.
 * test/serve-dist.ts (globalSetup) provides `glueUrl`, `wasmUrl` and
 * `dataUrl`, consumed via inject() in src/__tests__/browser.test.ts.
 */
import 'vitest'

declare module 'vitest' {
  export interface ProvidedContext {
    glueUrl: string
    wasmUrl: string
    dataUrl: string
  }
}
