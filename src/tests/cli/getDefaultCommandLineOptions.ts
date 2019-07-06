import { CommandLineOptions } from "../../cli";

export function getDefaultCommandLineOptions(): CommandLineOptions {
    return {
        showHelp: false,
        showVersion: false
    };
}