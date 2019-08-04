import { parseFile } from "./parsing";
import { print } from "./printing";
import { resolveNewLineKindFromText, ResolvedConfiguration } from "./configuration";
import { getFileKind } from "./getFileKind";
import { FileKind } from "./FileKind";

/**
 * Formats the provided text with the specified configuration.
 * @param filePath - File path of the text.
 * @param fileText - Text to format.
 * @param configuration - Configuration to use for formatting.
 */
export function formatFileText(filePath: string, fileText: string, configuration: ResolvedConfiguration) {
    const fileKind = getFileKind(filePath);
    const printItem = parseFile(fileKind, fileText, configuration);

    // the result was that it shouldn't be parsed so return the original text
    if (printItem === false)
        return fileText;

    return print(printItem, {
        newlineKind: configuration.newlineKind === "auto" ? resolveNewLineKindFromText(fileText) : configuration.newlineKind,
        maxWidth: fileKind === FileKind.Json ? configuration["json.lineWidth"] : configuration["typescript.lineWidth"],
        indentWidth: fileKind === FileKind.Json ? configuration["json.indentWidth"] : configuration["typescript.indentWidth"],
        useTabs: fileKind === FileKind.Json ? configuration["json.useTabs"] : configuration["typescript.useTabs"]
    });
}
