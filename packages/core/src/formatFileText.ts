import { print } from "./printing";
import { Plugin } from "./Plugin";
import { resolveNewLineKindFromText, throwError } from "./utils";

export interface FormatFileTextOptions {
    filePath: string;
    fileText: string;
    plugins: Plugin[];
}

export function formatFileText(options: FormatFileTextOptions) {
    const { filePath, fileText, plugins } = options;
    const plugin = getPlugin();

    // parse the file
    const parseResult = plugin.parseFile(filePath, fileText);
    if (!parseResult)
        return options.fileText;

    // print it
    const config = plugin.getConfiguration();
    return print(parseResult, {
        newlineKind: config.newlineKind === "auto" ? resolveNewLineKindFromText(fileText) : config.newlineKind,
        maxWidth: config.lineWidth,
        indentWidth: config.indentWidth,
        useTabs: config.useTabs
    });

    function getPlugin() {
        if (plugins.length === 0)
            return throwError("Formatter had zero plugins to format with. Did you mean to install or provide one such as dprint-plugin-typescript?");

        for (const plugin of plugins) {
            if (plugin.shouldParseFile(filePath, fileText))
                return plugin;
        }

        return throwError(`Could not find a plugin that would parse the file at path: ${filePath}`);
    }
}
