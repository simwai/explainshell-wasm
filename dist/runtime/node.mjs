import * as fs from 'node:fs';
import * as path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { i as isWasmGlueModule, p as parseExplainResult } from '../shared/explainshell-wasm.B0jjkJcj.mjs';

const __filename$1 = fileURLToPath(import.meta.url);
const __dirname$1 = path.dirname(__filename$1);
class NodeExplainshell {
  constructor(gluePath, dataPath) {
    this.gluePath = gluePath;
    this.dataPath = dataPath;
  }
  inner = null;
  async initialize() {
    if (this.inner) return;
    if (!fs.existsSync(this.gluePath) || !fs.existsSync(this.dataPath)) {
      throw new Error(
        `explainshell.wasm not built (missing ${this.gluePath} or ${this.dataPath}). Run: npm run build:wasm`
      );
    }
    const mod = await import(pathToFileURL(this.gluePath).href);
    const glue = mod.default ?? mod;
    if (!isWasmGlueModule(glue)) throw new Error("Invalid explainshell WASM glue module");
    const data = fs.readFileSync(this.dataPath);
    this.inner = new glue.Explainshell(data);
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
  const normalized = typeof options === "string" ? { dataPath: options } : options;
  const defaultGlue = path.resolve(__dirname$1, "../../dist/wasm-node/explainshell.js");
  const defaultData = path.resolve(__dirname$1, "../../dist/explainshell.data.msgpack");
  const finalGlue = normalized?.gluePath ?? defaultGlue;
  const finalData = normalized?.dataPath ?? defaultData;
  const key = `${finalGlue}::${finalData}`;
  if (!normalized?.forceNew && cachedInstance && cachedKey === key) return cachedInstance;
  cachedInstance?.terminate();
  const instance = new NodeExplainshell(finalGlue, finalData);
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

export { NodeExplainshell, createExplainshell, resetCache };
