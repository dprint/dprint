import { Project, TypeGuards, NewLineKind } from "ts-morph";

const readProject = new Project({ tsConfigFilePath: "tsconfig.json", compilerOptions: { declaration: true } });
const emitResult = readProject.emitToMemory({ emitOnlyDtsFiles: true });

for (const file of emitResult.getFiles())
    readProject.createSourceFile(file.filePath, file.text);

const emitMainFile = readProject.getSourceFileOrThrow("./dist/index.d.ts");
const writeProject = new Project({
    manipulationSettings: {
        newLineKind: NewLineKind.CarriageReturnLineFeed,
    },
});
const declarationFile = writeProject.addSourceFileAtPath("lib/dprint-types.d.ts");
const packageVersion = require("../package.json").version;

const writer = readProject.createWriter();
writer.writeLine("// dprint-ignore-file");

for (const [name, declarations] of emitMainFile.getExportedDeclarations()) {
    for (const declaration of declarations) {
        if (writer.getLength() > 0)
            writer.newLine();

        if (TypeGuards.isVariableDeclaration(declaration)) {
            // update to include the package version
            writer.write(declaration.getVariableStatementOrThrow().getText(true).replace("PACKAGE_VERSION", packageVersion));
        }
        else {
            writer.write(declaration.getText(true));
        }

        writer.newLine();
    }
}

// todo: format using dprint
declarationFile.replaceWithText(writer.toString());
declarationFile.saveSync();

const diagnostics = writeProject.getPreEmitDiagnostics();
if (diagnostics.length > 0) {
    console.log(writeProject.formatDiagnosticsWithColorAndContext(diagnostics));
    process.exit(1);
}
