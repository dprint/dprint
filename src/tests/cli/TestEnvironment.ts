import { Environment } from "../../cli";

export class TestEnvironment implements Environment {
    private readonly logs: string[] = [];
    private readonly files = new Map<string, string>();

    log(text: string) {
        this.logs.push(text);
    }

    getLogs() {
        return this.logs;
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

    resolvePath(path: string) {
        if (!path.startsWith("/"))
            path = "/" + path;
        return path;
    }
}
