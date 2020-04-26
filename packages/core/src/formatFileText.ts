import { Plugin } from "@dprint/types";
import { throwError } from "./utils";

/** Options for formatting. */
export interface FormatFileTextOptions {
    /** File path of the file to format. This will help select the plugin to use. */
    filePath: string;
    /** File text of the file to format. */
    fileText: string;
    /**
     * Plugins to use.
     * @remarks This function does not assume ownership of the plugins and so if there are
     * any plugins that require disposal then you should dispose of them after you no longer
     * need them.
     */
    plugins: Plugin[];
}

/**
 * Formats the provided file's text.
 * @param options - Options to use.
 * @returns The file text when it's changed; false otherwise.
 */
export function formatFileText(options: FormatFileTextOptions) {
    const { filePath, fileText, plugins } = options;
    const plugin = getPlugin();

    return plugin.formatText(filePath, fileText);

    function getPlugin() {
        if (plugins.length === 0)
            return throwError("Formatter had zero plugins to format with. Did you mean to install or provide one? (Ex. dprint-plugin-typescript)");

        for (const plugin of plugins) {
            if (plugin.shouldFormatFile(filePath, fileText))
                return plugin;
        }

        return throwError(`Could not find a plugin that would format the file at path: ${filePath}`);
    }
}
