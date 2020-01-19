import { Project, TypeGuards, NewLineKind } from "ts-morph";

const readProject = new Project({ tsConfigFilePath: "tsconfig.json", compilerOptions: { declaration: true } });
const emitResult = readProject.emitToMemory({ emitOnlyDtsFiles: true });

for (const file of emitResult.getFiles())
    readProject.createSourceFile(file.filePath, file.text, { overwrite: true });

const emitMainFile = readProject.getSourceFileOrThrow("./dist/index.d.ts");
const writeProject = new Project({
    manipulationSettings: {
        newLineKind: NewLineKind.CarriageReturnLineFeed
    }
});
const declarationFile = writeProject.addSourceFileAtPath("lib/dprint-core.d.ts");
const packageVersion = require("../package.json").version;

const writer = readProject.createWriter();

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
declarationFile.addImportDeclaration({
    namedImports: [
        "Condition",
        "Signal",
        "Info",
        "PrintItem",
        "PrintItemIterable",
        "WriterInfo",
        "Plugin",
        "Configuration",
        "ConfigurationDiagnostic",
        "ResolvedConfiguration",
        "ResolveConditionContext",
        "BaseResolvedConfiguration",
        "LoggingEnvironment"
    ],
    moduleSpecifier: "@dprint/types"
});
declarationFile.insertStatements(0, "// dprint-ignore-file");
declarationFile.saveSync();

const diagnostics = writeProject.getPreEmitDiagnostics();
if (diagnostics.length > 0) {
    console.log(writeProject.formatDiagnosticsWithColorAndContext(diagnostics));
    process.exit(1);
}
