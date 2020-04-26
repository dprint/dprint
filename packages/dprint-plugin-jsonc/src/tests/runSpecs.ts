import { runSpecs } from "@dprint/development";
import * as path from "path";
import { JsoncPlugin } from "../Plugin";

runSpecs({
    defaultFileName: "/file.json",
    specsDir: path.resolve(path.join(__dirname, "../../../rust-dprint-plugin-jsonc/tests/specs")),
    createPlugin: config => new JsoncPlugin(config as any),
});
