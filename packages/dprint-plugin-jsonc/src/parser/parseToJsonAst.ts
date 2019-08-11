import * as parser from "jsonc-parser";
import { formatJsonParserDiagnostics, throwError } from "../utils";

export function parseToJsonAst(fileText: string) {
    const diagnostics: parser.ParseError[] = [];
    const ast = parser.parseTree(fileText, diagnostics, {
        allowTrailingComma: true,
        disallowComments: false
    });

    if (diagnostics.length > 0)
        return throwError(`Encountered errors parsing document: ${formatJsonParserDiagnostics(diagnostics, fileText)}`);

    return ast;
}
