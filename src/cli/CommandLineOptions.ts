export interface CommandLineOptions {
    allowNodeModuleFiles: boolean;
    showHelp: boolean;
    showVersion: boolean;
    config: string | undefined;
    outputFilePaths: boolean;
    outputResolvedConfig: boolean;
    filePatterns: string[];
}
