import { getPackageVersion } from "./getPackageVersion";

export function getHelpText() {
    // I used tsc --help as an example template for this
    return `Version ${getPackageVersion()}
Syntax:   dprint [options] [...file patterns]
Examples: dprint
          dprint "src/**/*.ts"
Options:
-h, --help              Output this message.
-v, --version           Output the version.
-c, --config            Configuration file to use (default: dprint.config.js)
--outputFilePaths       Outputs the list of file paths found for formatting without formatting the files.
--outputResolvedConfig  Outputs the resolved configuration from the configuration file.
--duration              Outputs how long the format took.
--allowNodeModuleFiles  Allows including files that have a node_modules directory in their path.
`;
}
