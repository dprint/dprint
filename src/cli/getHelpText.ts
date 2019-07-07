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
-c, --config            Configuration file to use (default: dprint.config)
--outputFilePaths       Outputs the list of file paths.
--outputResolvedConfig  Outputs the resolved configuration from the dprint.config file.
`;
}
