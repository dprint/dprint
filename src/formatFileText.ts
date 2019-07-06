import { parseToBabelAst, parseFile, printParseTree } from "./parsing";
import { print } from "./printing";
import { resolveNewLineKindFromText, ResolvedConfiguration } from "./configuration";

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
