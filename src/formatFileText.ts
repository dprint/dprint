import { parseToBabelAst, parseFile } from "./parsing";
import { print } from "./printing";

export function formatFileText(filePath: string, fileText: string) {
    const babelAst = parseToBabelAst(filePath, fileText);
    const printItem = parseFile(babelAst, fileText, {
        newLineKind: "\n",
        semiColons: true,
        singleQuotes: false
    });

    return print(printItem, {
        maxWidth: 80,
        indentSize: 4,
        newLineKind: "\n"
    });
}
