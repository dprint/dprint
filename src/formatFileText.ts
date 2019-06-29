import { parseToBabelAst, parseFile, printParseTree } from "./parsing";
import { print } from "./printing";
import { resolveConfiguration, Configuration, resolveNewLineKindFromText } from "./configuration";

export function formatFileText(filePath: string, fileText: string, configuration?: Configuration) {
    // todo: use resolved configuration here
    configuration = configuration || {
        newLineKind: "lf",
        semiColons: true,
        singleQuotes: false,
        printWidth: 80
    };

    const babelAst = parseToBabelAst(filePath, fileText);
    const configurationResult = resolveConfiguration(configuration);
    const printItem = parseFile(babelAst, fileText, configurationResult.config);

    //console.log(printParseTree(printItem));
    //throw "STOP";

    return print(printItem, {
        maxWidth: configurationResult.config.printWidth,
        indentSize: configurationResult.config.indentSize,
        newLineKind: resolveNewLineKindFromText(fileText)
    });
}