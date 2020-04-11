import { Plugin, JsPlugin, WebAssemblyPlugin, isJsPlugin, BaseResolvedConfiguration } from "@dprint/types";
import { print } from "./printer";
import { resolveNewLineKindFromText, throwError, assertNever } from "./utils";

/** Options for formatting. */
export interface FormatFileTextOptions {
    /** File path of the file to format. This will help select the plugin to use. */
    filePath: string;
    /** File text of the file to format. */
    fileText: string;
    /**
     * Plugins to use.
     * @remarks This function does not assume ownership of the plugins and so if there are
     * any web assembly plugins you should dispose of them after you no longer need them.
     */
    plugins: Plugin[];
}

/**
 * Formats the provided file's text.
 * @param options - Options to use.
 */
export function formatFileText(options: FormatFileTextOptions) {
    const { filePath, fileText, plugins } = options;
    const plugin = getPlugin();

    return isJsPlugin(plugin) ? handleJsPlugin(plugin) : handleWebAssemblyPlugin(plugin);

    function handleJsPlugin(plugin: JsPlugin) {
        // parse the file
        const parseResult = plugin.parseFile(filePath, fileText);
        if (!parseResult)
            return options.fileText;

        // print it
        const config = plugin.getConfiguration();
        return print(parseResult, {
            newLineKind: resolveNewLineKind(config.newLineKind),
            maxWidth: config.lineWidth,
            indentWidth: config.indentWidth,
            useTabs: config.useTabs,
        });
    }

    function resolveNewLineKind(newLineKind: BaseResolvedConfiguration["newLineKind"]) {
        switch (newLineKind) {
            case "auto":
                return resolveNewLineKindFromText(fileText);
            case "crlf":
                return "\r\n";
            case "lf":
                return "\n";
            default:
                return assertNever(newLineKind);
        }
    }

    function handleWebAssemblyPlugin(plugin: WebAssemblyPlugin) {
        const formattedText = plugin.formatText(filePath, fileText);
        return formattedText === false ? options.fileText : formattedText;
    }

    function getPlugin() {
        if (plugins.length === 0)
            return throwError("Formatter had zero plugins to format with. Did you mean to install or provide one such as dprint-plugin-typescript?");

        for (const plugin of plugins) {
            if (plugin.shouldFormatFile(filePath, fileText))
                return plugin;
        }

        return throwError(`Could not find a plugin that would parse the file at path: ${filePath}`);
    }
}
