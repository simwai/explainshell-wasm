let cachedInstance = null;
async function createExplainshell(options) {
  if (cachedInstance && !options?.forceNew) return cachedInstance;
  const isNode = typeof process !== "undefined" && process.versions?.node;
  const isBrowser = typeof window !== "undefined" && typeof document !== "undefined";
  let runtime = options?.runtime || "auto";
  if (runtime === "auto") {
    runtime = isNode ? "node" : isBrowser ? "browser" : "node";
  }
  let instance;
  if (runtime === "node") {
    const { createExplainshell: createNodeExplainshell } = await import('./runtime/node.mjs');
    instance = await createNodeExplainshell({
      gluePath: options?.gluePath,
      dataPath: options?.dataPath,
      forceNew: options?.forceNew
    });
  } else if (runtime === "browser") {
    const { createExplainshell: createBrowserExplainshell } = await import('./runtime/browser.mjs');
    instance = await createBrowserExplainshell({
      glueUrl: options?.glueUrl,
      wasmUrl: options?.wasmUrl,
      dataUrl: options?.dataUrl,
      forceNew: options?.forceNew
    });
  } else {
    throw new Error(`Unknown runtime: ${runtime}`);
  }
  cachedInstance = instance;
  return instance;
}
async function explain(command, options) {
  const explainshell = await createExplainshell();
  return explainshell.explain(command, options);
}
function resetExplainshell() {
  cachedInstance?.terminate();
  cachedInstance = null;
}

export { createExplainshell, explain, resetExplainshell };
