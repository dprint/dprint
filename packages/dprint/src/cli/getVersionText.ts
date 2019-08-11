import { Plugin } from "@dprint/core";
import { getPackageVersion } from "./getPackageVersion";

export function getVersionText(plugins: Plugin[]) {
    return `dprint v${getPackageVersion()}${getPluginTexts()}`

    function getPluginTexts() {
        let result = "";

        if (plugins.length === 0)
            result += " (No plugins)";
        else {
            for (const plugin of plugins)
                result += `\n${plugin.name} v${plugin.version}`;
        }

        return result;
    }
}
