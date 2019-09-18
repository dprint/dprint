// @ts-check
// ===============
// To Local Source
// ===============
// Converts all the module specifiers referencing another package in the repository
// to point to the package's main TypeScript file itself. This is only meant as a
// temporary change to improve debugging between packages. For example, after doing
// this a break point could be placed in the core package and running the tests with
// debugging in dprint-plugin-typescript would hit that breakpoint.
//
// .USAGE
// yarn to-local-source
// yarn to-local-source --undo

const { Project } = require("ts-morph");

const project = new Project();

project.addSourceFilesFromTsConfig("packages/core/tsconfig.json");
project.addSourceFilesFromTsConfig("packages/development/tsconfig.json");
project.addSourceFilesFromTsConfig("packages/dprint/tsconfig.json");
project.addSourceFilesFromTsConfig("packages/dprint-plugin-jsonc/tsconfig.json");
project.addSourceFilesFromTsConfig("packages/dprint-plugin-typescript/tsconfig.json");

/** @type {[string, import("ts-morph").SourceFile][]} */
const mappings = [
    ["@dprint/core", project.getSourceFileOrThrow("packages/core/src/index.ts")],
    ["@dprint/development", project.getSourceFileOrThrow("packages/development/src/index.ts")]
];

if (process.argv[2] === "--undo")
    undoToLocalSource();
else
    toLocalSource();

project.save();

function toLocalSource() {
    for (const importDec of getAllImportDeclarations()) {
        const moduleSpecifierValue = importDec.getModuleSpecifierValue();
        for (const [packageName, sourceFile] of mappings) {
            if (packageName === moduleSpecifierValue)
                importDec.setModuleSpecifier(sourceFile);
        }
    }
}

function undoToLocalSource() {
    for (const importDec of getAllImportDeclarations()) {
        const moduleSpecifierSourceFile = importDec.getModuleSpecifierSourceFile();
        for (const [packageName, sourceFile] of mappings) {
            if (sourceFile === moduleSpecifierSourceFile)
                importDec.setModuleSpecifier(packageName);
        }
    }
}

function* getAllImportDeclarations() {
    for (const file of project.getSourceFiles())
        yield* file.getImportDeclarations();
}
