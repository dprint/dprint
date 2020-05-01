export interface CommandLineOptions {
    allowNodeModuleFiles: boolean;
    showHelp: boolean;
    showVersion: boolean;
    init: boolean;
    config: string | undefined;
    outputFilePaths: boolean;
    outputResolvedConfig: boolean;
    filePatterns: string[];
}
