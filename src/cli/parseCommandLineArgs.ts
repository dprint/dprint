import minimist from "minimist";
import { CommandLineOptions } from "./CommandLineOptions";

export function parseCommandLineArgs(args: string[]): CommandLineOptions {
    const argv = minimist(args, { boolean: true });

    return {
        showHelp: argv.hasOwnProperty("h") || argv.hasOwnProperty("help"),
        showVersion: argv.hasOwnProperty("v") || argv.hasOwnProperty("version"),
    };
}
