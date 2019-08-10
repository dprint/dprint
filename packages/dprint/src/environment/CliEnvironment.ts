import * as path from "path";
import globby from "globby";
import { CliLoggingEnvironment } from "@dprint/core";
import { Environment } from "./Environment";
import { readFile, writeFile } from "../utils";

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

    glob(patterns: string[]) {
        return globby(patterns, { absolute: true });
    }
}
