import { Plugin, PrintItemIterable, JsPlugin, WebAssemblyPlugin, isJsPlugin } from "@dprint/types";
import { print, PrintOptions } from "./printing";
import { resolveNewLineKindFromText, throwError } from "./utils";

/** Options for formatting. */
export interface FormatFileTextOptions {
    /** File path of the file to format. This will help select the plugin to use. */
    filePath: string;
    /** File text of the file to format. */
    fileText: string;
    /** Plugins to use. */
    plugins: Plugin[];
    /** Custom printer to print out the print items (ex. use the printer from @dprint/rust-printer) */
    customPrinter?: (iterable: PrintItemIterable, options: PrintOptions) => string;
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
        return (options.customPrinter || print)(parseResult, {
            newLineKind: config.newLineKind === "auto" ? resolveNewLineKindFromText(fileText) : config.newLineKind,
            maxWidth: config.lineWidth,
            indentWidth: config.indentWidth,
            useTabs: config.useTabs,
            isTesting: false // todo: make this true during testing (environment variable?)
        });
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
