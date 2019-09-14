import * as parser from "@babel/parser";
import { getFileExtension } from "@dprint/core";

export function parseToBabelAst(filePath: string, fileText: string) {
    return parser.parse(fileText, {
        sourceType: "module",
        tokens: true,
        plugins: Array.from(getPlugins()),
        // I implemented the library with this disabled,
        // but perhaps it should be enabled
        createParenthesizedExpressions: false,
        // be very relaxed
        allowAwaitOutsideFunction: true,
        allowImportExportEverywhere: true,
        allowReturnOutsideFunction: true,
        allowSuperOutsideMethod: true
    });

    function* getPlugins(): Iterable<parser.ParserPlugin> {
        if (isJsx())
            yield "jsx";

        yield "typescript";
        yield "bigInt";
        yield "classProperties";
        yield "decorators-legacy";
        yield "dynamicImport";
        yield "exportDefaultFrom";
        yield "exportNamespaceFrom";
        yield "importMeta";
        yield "nullishCoalescingOperator";
        yield "numericSeparator";
        yield "optionalChaining";

        function isJsx() {
            const fileExtension = getFileExtension(filePath).toLowerCase();
            return fileExtension === ".tsx" || fileExtension === ".jsx" || fileExtension === ".js";
        }
    }
}
