/** Represents an execution environment. */
export interface Environment {
    log(text: string): void;
    warn(text: string): void;
    error(text: string): void;
    resolvePath(path: string): string;
    readFile(filePath: string): Promise<string>;
    writeFile(filePath: string, text: string): Promise<void>;
    glob(patterns: string[]): Promise<string[]>;
}
