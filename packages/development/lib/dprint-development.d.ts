import { PrintItemIterable, Plugin } from "@dprint/types";

/** Prints out a parsed print item iterable for debugging purposes. */
export declare function getPrintIterableAsFormattedText(item: PrintItemIterable): string;

export declare function runSpecs(options: RunSpecsOptions): void;

export interface RunSpecsOptions {
    specsDir: string;
    createPlugin: (config: unknown) => Plugin;
    defaultFileName: string;
}
