import { runSpecs } from "@dprint/development";
import * as path from "path";
import { TypeScriptPlugin } from "../Plugin";

runSpecs({
    defaultFileName: "/file.ts",
    specsDir: path.resolve(path.join(__dirname, "specs")),
    createPlugin: config => new TypeScriptPlugin(config as any)
});
