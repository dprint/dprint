import { PrintItemIterator } from "../types";
import { parseToBabelAst, parseTypeScriptFile } from "./typescript";
import { ResolvedConfiguration } from "../configuration";
import { getFileExtension, throwError } from "../utils";

export function parseFile(filePath: string, fileText: string, configuration: ResolvedConfiguration): PrintItemIterator {
    const fileExtension = getFileExtension(filePath).toLowerCase();

    if (isTypeScriptFile()) {
        const babelAst = parseToBabelAst(filePath, fileText);
        return parseTypeScriptFile(babelAst, fileText, configuration);
    }
    else {
        return throwError(`Could not resolve parser based on file path: ${filePath}`);
    }

    function isTypeScriptFile() {
        switch (fileExtension) {
            case ".ts":
            case ".tsx":
            case ".js":
            case ".jsx":
                return true;
        }

        return false;
    }
}
