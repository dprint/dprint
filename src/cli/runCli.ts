import { parseCommandLineArgs } from "./parseCommandLineArgs";
import { getHelpText } from "./getHelpText";
import { getPackageVersion } from "./getPackageVersion";
import { CommandLineOptions } from "./CommandLineOptions";
import { Environment } from "./environment";

export function runCli(args: string[], environment: Environment) {
    const options = parseCommandLineArgs(args);
    handleCommandLineOptions(options, environment);
}

export function handleCommandLineOptions(options: CommandLineOptions, environment: Environment) {
    if (options.showHelp)
        environment.log(getHelpText());
    else if (options.showVersion)
        environment.log(getPackageVersion());
}
