import * as path from "path";
import globby from "globby";
import { CliLoggingEnvironment } from "@dprint/core";
import { Environment } from "./Environment";
import { readFile, writeFile, exists } from "../utils";

/**
 * An implementation of an environment that interacts with the user's file system and outputs to the console.
 */
export class CliEnvironment extends CliLoggingEnvironment implements Environment {
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

    exists(filePath: string) {
        return exists(filePath);
    }

    glob(patterns: string[]) {
        return globby(patterns, { absolute: true });
    }

    require(filePath: string) {
        // todo: use a dynamic import here?
        return new Promise<unknown>((resolve, reject) => {
            try {
                resolve(require(filePath));
            } catch (err) {
                reject(err);
            }
        });
    }
}
