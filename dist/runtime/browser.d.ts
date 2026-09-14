import { ExplainshellInstance, ExplainOptions, ExplainResult } from '../index.js';

declare class BrowserExplainshell implements ExplainshellInstance {
    private glueUrl;
    private wasmUrl;
    private dataUrl;
    private inner;
    constructor(glueUrl: string, wasmUrl: string, dataUrl: string);
    initialize(): Promise<void>;
    private requireInner;
    explain(command: string, _options?: ExplainOptions): Promise<ExplainResult>;
    explainAst(astJson: string, _options?: ExplainOptions): Promise<ExplainResult>;
    manpageCount(): number;
    terminate(): void;
}
interface BrowserLoaderOptions {
    glueUrl?: string;
    wasmUrl?: string;
    dataUrl?: string;
    forceNew?: boolean;
}
declare function createExplainshell(options?: string | BrowserLoaderOptions): Promise<ExplainshellInstance>;
declare function resetCache(): void;

export { BrowserExplainshell, createExplainshell, resetCache };
export type { BrowserLoaderOptions };
