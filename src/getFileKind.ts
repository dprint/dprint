import { getFileExtension, throwError } from "./utils";
import { FileKind } from "./FileKind";

export function getFileKind(filePath: string) {
    const fileExtension = getFileExtension(filePath).toLowerCase();

    if (fileExtension === ".ts" || fileExtension === ".js")
        return FileKind.TypeScript;
    else if (fileExtension === ".tsx" || fileExtension === ".jsx")
        return FileKind.TypeScriptTsx;
    else if (fileExtension === ".json")
        return FileKind.Json;
    else
        return throwError(`Could not resolve file kind based on file path: ${filePath}`);
}
