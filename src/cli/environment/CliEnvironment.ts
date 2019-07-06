import * as path from "path";
import { Environment } from "./Environment";
import { readFile } from "../../utils";

export class CliEnvironment implements Environment {
    readFile(filePath: string) {
        return readFile(filePath);
    }

    log(text: string) {
        console.log(text);
    }

    resolvePath(fileOrDirPath: string) {
        return path.normalize(path.resolve(fileOrDirPath));
    }
}
