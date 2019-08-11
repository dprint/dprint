import { Project, TypeGuards, NewLineKind } from "ts-morph";

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
const declarationFile = writeProject.addExistingSourceFile("lib/dprint-core.d.ts");
const packageVersion = require("../package.json").version;

let text = "";

for (const [name, declarations] of emitMainFile.getExportedDeclarations()) {
    for (const declaration of declarations) {
        if (text.length > 0)
            text += "\n";

        if (TypeGuards.isVariableDeclaration(declaration)) {
            // update to include the package version
            text += declaration.getVariableStatementOrThrow().getText(true).replace("PACKAGE_VERSION", packageVersion);
        }
        else {
            text += declaration.getText(true);
        }

        text += "\n";
    }
}

// todo: format using dprint
declarationFile.replaceWithText(text);
declarationFile.saveSync();

const diagnostics = writeProject.getPreEmitDiagnostics();
if (diagnostics.length > 0) {
    console.log(writeProject.formatDiagnosticsWithColorAndContext(diagnostics));
    process.exit(1);
}
