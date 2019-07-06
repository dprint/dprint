export interface CommandLineOptions {
    showHelp: boolean;
    showVersion: boolean;
    config: string | undefined;
    outputFilePaths: boolean;
    filePatterns: string[];
}
