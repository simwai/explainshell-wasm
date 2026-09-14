import { i as isWasmGlueModule, p as parseExplainResult } from '../shared/explainshell-wasm.B0jjkJcj.mjs';

class BrowserExplainshell {
  constructor(glueUrl, wasmUrl, dataUrl) {
    this.glueUrl = glueUrl;
    this.wasmUrl = wasmUrl;
    this.dataUrl = dataUrl;
  }
  inner = null;
  async initialize() {
    if (this.inner) return;
    const mod = await import(
      /* @vite-ignore */
      this.glueUrl
    );
    if (!isWasmGlueModule(mod)) throw new Error("Invalid explainshell WASM glue module");
    if (typeof mod.default === "function") {
      await mod.default(this.wasmUrl);
    }
    const response = await fetch(this.dataUrl);
    if (!response.ok) {
      throw new Error(`Failed to fetch data bundle: ${response.status} ${response.statusText}`);
    }
    const data = new Uint8Array(await response.arrayBuffer());
    this.inner = new mod.Explainshell(data);
  }
  requireInner() {
    if (!this.inner) throw new Error("Not initialized");
    return this.inner;
  }
  async explain(command, _options) {
    if (!this.inner) await this.initialize();
    return parseExplainResult(this.requireInner().explain(command));
  }
  async explainAst(astJson, _options) {
    if (!this.inner) await this.initialize();
    return parseExplainResult(this.requireInner().explain_ast(astJson));
  }
  manpageCount() {
    return this.requireInner().manpage_count();
  }
  terminate() {
    this.inner?.free();
    this.inner = null;
  }
}
let cachedInstance = null;
let cachedKey = null;
async function createExplainshell(options) {
  const normalized = typeof options === "string" ? { dataUrl: options } : options;
  const finalGlue = normalized?.glueUrl ?? "/wasm-web/explainshell.js";
  const finalWasm = normalized?.wasmUrl ?? "/wasm-web/explainshell_bg.wasm";
  const finalData = normalized?.dataUrl ?? "/explainshell.data.msgpack";
  const key = `${finalGlue}::${finalWasm}::${finalData}`;
  if (!normalized?.forceNew && cachedInstance && cachedKey === key) return cachedInstance;
  cachedInstance?.terminate();
  const instance = new BrowserExplainshell(finalGlue, finalWasm, finalData);
  await instance.initialize();
  cachedInstance = instance;
  cachedKey = key;
  return instance;
}
function resetCache() {
  cachedInstance?.terminate();
  cachedInstance = null;
  cachedKey = null;
}

export { BrowserExplainshell, createExplainshell, resetCache };
