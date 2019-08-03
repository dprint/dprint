import * as path from "path";
import globby from "globby";
import { Environment } from "./Environment";
import { readFile, writeFile } from "../utils";

/**
 * An implementation of an environment that interacts with the user's file system and outputs to the console.
 */
export class RealEnvironment implements Environment {
    log(text: string) {
        console.log(text);
    }

    warn(text: string) {
        console.warn(text);
    }

    error(text: string) {
        console.error(text);
    }

    basename(fileOrDirPath: string) {
        return path.basename(fileOrDirPath);
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
