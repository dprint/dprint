import { PrintItemIterator } from "../types";
import { ResolvedConfiguration } from "../configuration";
import { FileKind } from "../FileKind";
import { assertNever } from "../utils";
import { parseToBabelAst, parseTypeScriptFile } from "./typescript";
import { parseToJsonAst, parseJsonFile } from "./json";

export function parseFile(fileKind: FileKind, fileText: string, configuration: ResolvedConfiguration): PrintItemIterator | false {
    if (fileKind === FileKind.TypeScript || fileKind === FileKind.TypeScriptTsx) {
        const babelAst = parseToBabelAst(fileKind, fileText);
        return parseTypeScriptFile(babelAst, fileText, configuration);
    }
    else if (fileKind === FileKind.Json) {
        const jsonAst = parseToJsonAst(fileText);
        return parseJsonFile(jsonAst, fileText, configuration);
    }
    else {
        return assertNever(fileKind);
    }
}
