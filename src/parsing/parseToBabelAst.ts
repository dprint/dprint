import * as parser from "@babel/parser";

export function parseToBabelAst(fileName: string, code: string) {
    return parser.parse(code, {
        sourceFilename: fileName,
        sourceType: "module",
        tokens: true,
        plugins: [
            "jsx",
            "typescript",
            "bigInt",
            "classProperties",
            "decorators-legacy",
            "exportDefaultFrom",
            "exportNamespaceFrom",
            "optionalChaining"
        ]
    });
}
