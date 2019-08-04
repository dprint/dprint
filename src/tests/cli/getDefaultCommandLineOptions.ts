import { CommandLineOptions } from "../../cli";

export function getDefaultCommandLineOptions(): CommandLineOptions {
    return {
        allowNodeModuleFiles: false,
        config: undefined,
        showHelp: false,
        showVersion: false,
        outputFilePaths: false,
        outputResolvedConfig: false,
        duration: false,
        filePatterns: []
    };
}
