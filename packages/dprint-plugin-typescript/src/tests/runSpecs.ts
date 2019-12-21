import { runSpecs } from "@dprint/development";
import * as path from "path";
import { TypeScriptPlugin } from "../Plugin";

// temporary code as this transitions to rust

runSpecs({
    defaultFileName: "/file.ts",
    specsDir: path.resolve(path.join(__dirname, "./specs")),
    createPlugin: config => new TypeScriptPlugin(config as any)
});

runSpecs({
    defaultFileName: "/file.ts",
    specsDir: path.resolve(path.join(__dirname, "../../../rust-dprint-plugin-typescript/tests/specs")),
    createPlugin: config => new TypeScriptPlugin(config as any)
});