import { runSpecs } from "@dprint/development";
import * as path from "path";
import { default as TypeScriptPlugin } from "../Plugin";

runSpecs({
    defaultFileName: "/file.ts",
    specsDir: path.resolve(path.join(__dirname, "specs")),
    plugin: new TypeScriptPlugin()
});
