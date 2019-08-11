import { Project, TypeGuards, StructureKind, NewLineKind } from "ts-morph";

const readProject = new Project({ tsConfigFilePath: "tsconfig.json" });
const emitResult = readProject.emitToMemory({ emitOnlyDtsFiles: true });

for (const file of emitResult.getFiles())
    readProject.createSourceFile(file.filePath, file.text);

const emitMainFile = readProject.getSourceFileOrThrow("./dist/index.d.ts");
const writeProject = new Project({
    manipulationSettings: {
        newLineKind: NewLineKind.CarriageReturnLineFeed
    }
});
const declarationFile = writeProject.addExistingSourceFile("lib/dprint.d.ts");

let text = "";

for (const [name, declarations] of emitMainFile.getExportedDeclarations()) {
    for (const declaration of declarations) {
        if (text.length > 0)
            text += "\n";

        if (name === "Configuration") {
            text += `export { Configuration } from "@dprint/core";`;
            break;
        }

        if (TypeGuards.isVariableDeclaration(declaration))
            text += declaration.getVariableStatementOrThrow().getText(true);
        else
            text += declaration.getText(true);

        text += "\n";
    }
}

// todo: format using dprint
declarationFile.replaceWithText(text);
declarationFile.insertStatements(0, [{
    kind: StructureKind.ImportDeclaration,
    namedImports: ["LoggingEnvironment", "CliLoggingEnvironment"],
    moduleSpecifier: "@dprint/core"
}, {
    kind: StructureKind.ExportDeclaration,
    namedExports: ["Configuration"],
    moduleSpecifier: "@dprint/core"
}]);
declarationFile.saveSync();

const diagnostics = writeProject.getPreEmitDiagnostics();
if (diagnostics.length > 0) {
    console.log(writeProject.formatDiagnosticsWithColorAndContext(diagnostics));
    process.exit(1);
}
