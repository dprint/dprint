import * as path from "path";
import { CliLoggingEnvironment } from "@dprint/core";
import { Environment } from "./Environment";
import { readFile, writeFile, exists, rename, unlink, unlinkSync } from "../utils";

/**
 * An implementation of an environment that interacts with the user's file system and outputs to the console.
 */
export class CliEnvironment extends CliLoggingEnvironment implements Environment {
    // prevents loading this in environments that don't support it
    /** @internal */
    private fastGlob: typeof import("fast-glob") = require("fast-glob");

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
        return this.fastGlob(backSlashesToForward(patterns), {
            absolute: true,
            cwd: path.resolve("."),
        });
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

    rename(oldFilePath: string, newFilePath: string) {
        return rename(oldFilePath, newFilePath);
    }

    unlink(filePath: string) {
        return unlink(filePath);
    }

    unlinkSync(filePath: string) {
        unlinkSync(filePath);
    }
}

function backSlashesToForward(patterns: ReadonlyArray<string>) {
    return patterns.map(p => p.replace(/\\/g, "/")); // maybe this isn't full-proof?
}
