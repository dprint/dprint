import * as parser from "jsonc-parser";
import { throwError } from "../../utils";

export function parseToJsonAst(code: string) {
    // todo: configuration to use trailing comments
    const errors: parser.ParseError[] = [];
    const ast = parser.parseTree(code, errors, {
        allowTrailingComma: true,
        disallowComments: false
    });

    if (errors.length > 0)
        return throwError(`Encountered errors parsing document: ${errors.map(e => e.toString())}`);

    return ast;
}
