import { Project, TypeGuards } from "ts-morph";

const readProject = new Project({ tsConfigFilePath: "tsconfig.json", compilerOptions: { declaration: true } });
const emitResult = readProject.emitToMemory({ emitOnlyDtsFiles: true });

for (const file of emitResult.getFiles())
    readProject.createSourceFile(file.filePath, file.text, { overwrite: true });

const emitMainFile = readProject.getSourceFileOrThrow("./dist/index.d.ts");
const writeProject = new Project();
const declarationFile = writeProject.addSourceFileAtPath("lib/dprint-plugin-typescript.d.ts");

let text = "";

for (const [name, declarations] of emitMainFile.getExportedDeclarations()) {
    for (const declaration of declarations) {
        if (text.length > 0)
            text += "\n";

        if (TypeGuards.isVariableDeclaration(declaration))
            text += declaration.getVariableStatementOrThrow().getText(true);
        else
            text += declaration.getText(true);

        text += "\n";
    }
}

// todo: format using dprint
declarationFile.replaceWithText(text);
declarationFile.insertImportDeclaration(0, {
    namedImports: ["Plugin", "PluginInitializeOptions", "BaseResolvedConfiguration", "ConfigurationDiagnostic"],
    moduleSpecifier: "@dprint/types",
});
declarationFile.saveSync();

const diagnostics = writeProject.getPreEmitDiagnostics();
if (diagnostics.length > 0) {
    console.log(writeProject.formatDiagnosticsWithColorAndContext(diagnostics));
    process.exit(1);
}
