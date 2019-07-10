import { parseToBabelAst, parseFile, printParseTree } from "./parsing";
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

    // console.log(printParseTree(printItem));
    // throw "STOP";

    return print(printItem, {
        maxWidth: configuration.lineWidth,
        indentSize: configuration.indentSize,
        newLineKind: configuration.newLineKind === "auto" ? resolveNewLineKindFromText(fileText) : configuration.newLineKind,
        useTabs: configuration.useTabs
    });
}
