import * as path from "path";
import multimatch from "multimatch";
import { Environment } from "../../environment";

export class TestEnvironment implements Environment {
    private readonly logs: string[] = [];
    private readonly warns: string[] = [];
    private readonly errors: string[] = [];
    private readonly files = new Map<string, string>();
    private readonly requireObjects = new Map<string, object>();

    log(text: string) {
        this.logs.push(text);
    }

    warn(text: string) {
        this.warns.push(text);
    }

    error(text: string) {
        this.errors.push(text);
    }

    getLogs() {
        return this.logs;
    }

    getWarns() {
        return this.warns;
    }

    getErrors() {
        return this.errors;
    }

    addFile(filePath: string, text: string) {
        this.files.set(filePath, text);
    }

    readFile(filePath: string) {
        const fileText = this.files.get(filePath);
        if (fileText == null)
            return Promise.reject(new Error("File not found."));

        return Promise.resolve(fileText);
    }

    writeFile(filePath: string, text: string) {
        this.files.set(filePath, text);
        return Promise.resolve();
    }

    exists(filePath: string) {
        return Promise.resolve(this.files.has(filePath));
    }

    basename(fileOrDirPath: string) {
        return path.basename(fileOrDirPath);
    }

    resolvePath(fileOrDirPath: string) {
        if (!fileOrDirPath.startsWith("/"))
            fileOrDirPath = "/" + fileOrDirPath;
        return fileOrDirPath;
    }

    setRequireObject(filePath: string, value: object) {
        this.requireObjects.set(filePath, value);
    }

    removeRequireObject(filePath: string) {
        this.requireObjects.delete(filePath);
    }

    require(filePath: string): Promise<unknown> {
        if (!this.requireObjects.has(filePath))
            return Promise.reject(new Error("File not found."));

        return Promise.resolve(this.requireObjects.get(filePath)!);
    }

    glob(patterns: string[]) {
        return Promise.resolve(matchGlobs(Array.from(this.files.keys()), patterns));
    }

    rename(oldFilePath: string, newFilePath: string) {
        const fileText = this.files.get(oldFilePath);
        this.files.delete(oldFilePath);
        this.files.set(newFilePath, fileText || "");
        return Promise.resolve();
    }

    unlinkSync(filePath: string) {
        this.files.delete(filePath);
    }

    unlink(filePath: string) {
        this.unlinkSync(filePath);
        return Promise.resolve();
    }
}

function matchGlobs(paths: ReadonlyArray<string>, patterns: ReadonlyArray<string>) {
    // @types/multimatch incorrectly specifies `string[]` type despite not modifying the array
    return multimatch(paths as string[], patterns as string[]);
}
