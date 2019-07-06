import { CommandLineOptions } from "../../cli";

export function getDefaultCommandLineOptions(): CommandLineOptions {
    return {
        config: undefined,
        showHelp: false,
        showVersion: false,
        outputFilePaths: false,
        filePatterns: []
    };
}