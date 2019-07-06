import * as path from "path";
import globby from "globby";
import { Environment } from "./Environment";
import { readFile, writeFile } from "../../utils";

export class CliEnvironment implements Environment {
    log(text: string) {
        console.log(text);
    }

    warn(text: string) {
        console.warn(text);
    }

    error(text: string) {
        console.error(text)
    }

    resolvePath(fileOrDirPath: string) {
        return path.normalize(path.resolve(fileOrDirPath));
    }

    readFile(filePath: string) {
        return readFile(filePath);
    }

    writeFile(filePath: string, text: string) {
        return writeFile(filePath, text);
    }

    glob(patterns: string[]) {
        return globby(patterns, { absolute: true });
    }
}
