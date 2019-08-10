import { runSpecs } from "@dprint/development";
import * as path from "path";
import { default as JsoncPlugin } from "../Plugin";

runSpecs({
    defaultFileName: "/file.json",
    specsDir: path.resolve(path.join(__dirname, "specs")),
    plugin: new JsoncPlugin()
});
