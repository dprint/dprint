import * as parser from "@babel/parser";
import { FileKind } from "../../FileKind";

export function parseToBabelAst(fileKind: FileKind.TypeScript | FileKind.TypeScriptTsx, fileText: string) {
    return parser.parse(fileText, {
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
        if (fileKind === FileKind.TypeScriptTsx)
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
}
