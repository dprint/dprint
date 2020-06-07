export interface Formatter {
    formatText(fileExtension: string, text: string): string;
    setConfig(configText: string): void;
    getFileExtensions(): string[];
    getConfigSchemaUrl(): string;
}
