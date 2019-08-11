import { LoggingEnvironment } from "@dprint/core";

/** Represents an execution environment. */
export interface Environment extends LoggingEnvironment {
    resolvePath(path: string): string;
    basename(filePath: string): string;
    readFile(filePath: string): Promise<string>;
    writeFile(filePath: string, text: string): Promise<void>;
    exists(filePath: string): Promise<boolean>;
    glob(patterns: string[]): Promise<string[]>;
    require(path: string): Promise<unknown>;
}
