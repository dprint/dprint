import * as parser from "@babel/parser";

const isJsxExtension = /\.(j|t)sx$/i;

export function parseToBabelAst(fileName: string, code: string) {
    return parser.parse(code, {
        sourceFilename: fileName,
        sourceType: "module",
        tokens: true,
        plugins: Array.from(getPlugins()),
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
        yield "optionalChaining";
    }

    function isJsx() {
        return isJsxExtension.test(fileName);
    }
}
