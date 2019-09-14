export interface CommandLineOptions {
    allowNodeModuleFiles: boolean;
    showHelp: boolean;
    showVersion: boolean;
    init: boolean;
    config: string | undefined;
    outputFilePaths: boolean;
    outputResolvedConfig: boolean;
    /** Specifies whether to output the duration or not. */
    duration: boolean;
    filePatterns: string[];
}
