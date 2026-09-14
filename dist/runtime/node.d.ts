import { ExplainshellInstance, ExplainOptions, ExplainResult } from '../index.js';

declare class NodeExplainshell implements ExplainshellInstance {
    private gluePath;
    private dataPath;
    private inner;
    constructor(gluePath: string, dataPath: string);
    initialize(): Promise<void>;
    private requireInner;
    explain(command: string, _options?: ExplainOptions): Promise<ExplainResult>;
    explainAst(astJson: string, _options?: ExplainOptions): Promise<ExplainResult>;
    manpageCount(): number;
    terminate(): void;
}
interface NodeLoaderOptions {
    gluePath?: string;
    dataPath?: string;
    forceNew?: boolean;
}
declare function createExplainshell(options?: string | NodeLoaderOptions): Promise<ExplainshellInstance>;
declare function resetCache(): void;

export { NodeExplainshell, createExplainshell, resetCache };
export type { NodeLoaderOptions };
