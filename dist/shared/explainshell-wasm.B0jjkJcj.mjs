function isWasmGlueModule(value) {
  return typeof value === "object" && value !== null && "Explainshell" in value && typeof value.Explainshell === "function";
}
function isRecord(value) {
  return typeof value === "object" && value !== null;
}
function parseExplainResult(json) {
  const result = JSON.parse(json);
  if (!isRecord(result)) throw new Error("Unexpected response format");
  if (!("groups" in result) || !Array.isArray(result.groups)) {
    throw new Error("Unexpected response format");
  }
  for (const group of result.groups) {
    if (!isRecord(group) || typeof group.name !== "string") {
      throw new Error("Unexpected response format");
    }
    if (!("results" in group) || !Array.isArray(group.results)) {
      throw new Error("Unexpected response format");
    }
    for (const r of group.results) {
      if (!isRecord(r) || typeof r.start !== "number" || typeof r.end !== "number") {
        throw new Error("Unexpected response format");
      }
    }
  }
  if ("expansions" in result && !Array.isArray(result.expansions)) {
    throw new Error("Unexpected response format");
  }
  return result;
}

export { isWasmGlueModule as i, parseExplainResult as p };
