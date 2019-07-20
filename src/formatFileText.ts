import { parseToBabelAst, parseFile } from "./parsing";
import { print } from "./printing";
import { resolveNewLineKindFromText, ResolvedConfiguration } from "./configuration";

/**
 * Formats the provided text with the specified configuration.
 * @param filePath - File path of the text.
 * @param fileText - Text to format.
 * @param configuration - Configuration to use for formatting.
 */
export function formatFileText(filePath: string, fileText: string, configuration: ResolvedConfiguration) {
    const babelAst = parseToBabelAst(filePath, fileText);
    const printItem = parseFile(babelAst, fileText, configuration);

    return print(printItem, {
        maxWidth: configuration.lineWidth,
        indentSize: configuration.indentSize,
        newlineKind: configuration.newlineKind === "auto" ? resolveNewLineKindFromText(fileText) : configuration.newlineKind,
        useTabs: configuration.useTabs
    });
}
