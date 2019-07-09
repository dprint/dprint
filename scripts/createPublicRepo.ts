import * as path from "path";
import { Project } from "ts-morph";

const project = new Project();
const fileSystem = project.getFileSystem();

const copyToDir = "./public_test";

// todo: clear out existing files
copy("dprint.config");
copy("implemented-nodes.md");
copy("mocha.opts");
copy("package.json"); // todo: strip this down using a whitelist of properties to keep
copy("README.md");
copy("tsconfig.common.json");
copy("tsconfig.json");
copy("dist/cli-bin.js");
copy("dist/dprint.js");
copy("schema");
copy("docs");
copy("lib");
copy("src/tests/specs");
copy("src/tests/runSpecs.ts");
copy("src/tests/specParser.ts");
copy("public/public-package.json", "package.json");

function copy(filePath: string, fileTo?: string) {
    fileTo = fileTo || filePath;
    fileSystem.copySync(filePath, path.join(copyToDir, fileTo));
}
