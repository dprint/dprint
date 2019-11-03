import { Plugin } from "@dprint/types";
import { getPackageVersion } from "./getPackageVersion";

export function getHelpText(plugins: Plugin[]) {
    // I used tsc --help as an example template for this
    return `dprint v${getPackageVersion()}

Syntax:   dprint [options] [...file patterns]
Examples: dprint
          dprint "src/**/*.ts"
Options:
-h, --help              Outputs this message.
-v, --version           Outputs the version of the library and plugins.
--init                  Creates a dprint.config.js file in the current directory.
-c, --config            Configuration file to use (default: dprint.config.js)
--outputFilePaths       Outputs the list of file paths found for formatting without formatting the files.
--outputResolvedConfig  Outputs the resolved configuration from the configuration file.
--duration              Outputs how long the format took.
--allowNodeModuleFiles  Allows including files that have a node_modules directory in their path.
${getPluginTexts()}`;

    function getPluginTexts() {
        const prefix = "Plugins:";
        let result = prefix;

        if (plugins.length === 0)
            result += " [No plugins]";
        else {
            for (const plugin of plugins)
                result += `\n* ${plugin.name} v${plugin.version}`;
        }

        return result;
    }
}
