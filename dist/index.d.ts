import { E as ExplainshellInstance, a as ExplainOptions, b as ExplainResult } from './shared/explainshell-wasm.BrpcExcK.js';
export { C as CliOption, c as Expansion, d as ExplainResultGroup, M as MatchGroup, e as MatchResult, P as ParsedManpage } from './shared/explainshell-wasm.BrpcExcK.js';

interface CreateExplainshellOptions {
    gluePath?: string;
    dataPath?: string;
    glueUrl?: string;
    wasmUrl?: string;
    dataUrl?: string;
    runtime?: 'node' | 'browser' | 'auto';
    forceNew?: boolean;
}
declare function createExplainshell(options?: CreateExplainshellOptions): Promise<ExplainshellInstance>;
/** One-shot explain using the cached instance. */
declare function explain(command: string, options?: ExplainOptions): Promise<ExplainResult>;
declare function resetExplainshell(): void;

export { ExplainOptions, ExplainResult, ExplainshellInstance, createExplainshell, explain, resetExplainshell };
