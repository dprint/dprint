import * as parser from "@babel/parser";

export function parseToBabelAst(fileName: string, code: string) {
    return parser.parse(code, {
        sourceFilename: fileName,
        sourceType: "module",
        plugins: [
            "jsx",
            "typescript",
            "bigInt",
            "optionalChaining"
        ]
    });
}
