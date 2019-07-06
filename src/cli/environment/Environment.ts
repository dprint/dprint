export interface Environment {
    readFile(text: string): Promise<string>;
    log(text: string): void;
    resolvePath(path: string): string;
}
