/**
 * explainshell-wasm - explainshell compiled to WebAssembly
 *
 * @packageDocumentation
 */

export {
  createExplainshell,
  explain,
  resetExplainshell,
} from './api.js'

export type {
  CliOption,
  ExplainOptions,
  ExplainResult,
  ExplainResultGroup,
  Expansion,
  ExplainshellInstance,
  MatchGroup,
  MatchResult,
  ParsedManpage,
} from './types.js'
