import { Plugin } from "@dprint/types";

export declare function runSpecs(options: RunSpecsOptions): void;

export interface RunSpecsOptions {
    specsDir: string;
    createPlugin: (config: unknown) => Plugin;
    defaultFileName: string;
}
