/**
 * TypeScript types for explainshell-wasm.
 *
 * These mirror the Rust types in crates/core (serde snake_case wire format)
 * as produced by Explainshell.explain(), which returns an ExplainResult
 * JSON string parsed and validated by src/runtime/utils.ts.
 */
/** Options for explain(). Reserved for multi-distro bundles; accepted but currently unused. */
interface ExplainOptions {
    /** Distribution filter, e.g. 'ubuntu' */
    distro?: string;
    /** Release filter, e.g. '26.04' */
    release?: string;
}
/** An extracted command-line option from a man page. */
interface CliOption {
    /** Human-readable help text */
    text: string;
    /** Short options, e.g. ["-v"] */
    short: string[];
    /** Long options, e.g. ["--verbose"] */
    long: string[];
    /** Whether the option expects an argument (or the list of possible values) */
    has_argument: boolean | string[];
    /** Positional argument name, or boolean flag */
    positional?: string | boolean | null;
    /** Literal prefix sigil a token must carry (e.g. "@" for dig @server) */
    prefix?: string | null;
    /** Whether the option can start a nested command (or its end-words) */
    nested_cmd: boolean | string | string[];
    /** Arbitrary extraction metadata */
    meta?: Record<string, unknown> | null;
}
/** A processed man page with extracted options. */
interface ParsedManpage {
    /** Source path, e.g. "ubuntu/26.04/1/tar.1.gz" */
    source: string;
    /** Command name, e.g. "tar" */
    name: string;
    /** One-line synopsis */
    synopsis?: string | null;
    /** Extracted options */
    options: CliOption[];
    /** Aliases with scores */
    aliases: [string, number][];
    /** Allow matching options without a leading dash */
    dashless_opts: boolean;
    /** Subcommand names, e.g. ["commit", "push"] */
    subcommands: string[];
    /** Manually updated flag */
    updated: boolean;
    /** Positional arguments can start a nested command */
    nested_cmd: boolean | string | string[];
    /** Extractor identifier, e.g. "llm" */
    extractor?: string | null;
}
/** A single matched token span in the input command line. */
interface MatchResult {
    /** Start position in the input string */
    start: number;
    /** End position in the input string (exclusive) */
    end: number;
    /** Help text from the manpage option, or null when unknown */
    text?: string | null;
    /** The matched portion of the input string */
    match_text?: string | null;
    /** Debug metadata describing the match kind */
    debug_info?: Record<string, unknown> | null;
}
/** A group of match results: one shell group plus one per command. */
interface MatchGroup {
    /** Group name, e.g. "shell", "command1" */
    name: string;
    /** Match results in this group */
    results: MatchResult[];
    /** Associated manpage, when the command resolved to one */
    manpage?: ParsedManpage | null;
    /** Alternative manpage suggestions */
    suggestions?: ParsedManpage[];
    /** Error text when the command resolved to no manpage */
    error?: string | null;
    /** How many ordered positionals this group has consumed */
    positional_index: number;
}
/** Word expansion detected during parsing. */
interface Expansion {
    start: number;
    end: number;
    /** e.g. "substitution", "tilde", "parameter-star" */
    kind: string;
}
/** Complete explanation of a command line. */
interface ExplainResult {
    /** All match groups */
    groups: ExplainResultGroup[];
    /** Expansions detected during parsing */
    expansions: Expansion[];
}
type ExplainResultGroup = MatchGroup;
/** A live explainshell engine instance. */
interface ExplainshellInstance {
    /** Explain a shell command */
    explain(command: string, options?: ExplainOptions): Promise<ExplainResult>;
    /** Explain from a pre-parsed AST JSON string (bashlex-compatible) */
    explainAst(astJson: string, options?: ExplainOptions): Promise<ExplainResult>;
    /** Number of manpages in the loaded bundle */
    manpageCount(): number;
    /** Release the instance */
    terminate(): void;
}

export type { CliOption as C, ExplainshellInstance as E, MatchGroup as M, ParsedManpage as P, ExplainOptions as a, ExplainResult as b, Expansion as c, ExplainResultGroup as d, MatchResult as e };
