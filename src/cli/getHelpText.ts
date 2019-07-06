import { getPackageVersion } from "./getPackageVersion";

export function getHelpText() {
    // I used tsc --help as an example template for this
    return `Version ${getPackageVersion()}
Syntax:   dprint [options] "[glob]"
Examples: dprint
          dprint "src/**/*.ts"
Options:
-h, --help       Output this message.
-v, --version    Output the version.
`;
}
