import { Project } from "ts-morph";

const project = new Project();
const sourceFile = project.addSourceFileAtPath("src/cli-bin.ts");
const importDec = sourceFile.getImportDeclarationOrThrow("./index");

importDec.setModuleSpecifier("./dprint");

const emitOutput = sourceFile.getEmitOutput();
const emitFile = emitOutput.getOutputFiles().find(f => f.getFilePath().endsWith("cli-bin.js"))!;

project.getFileSystem().writeFileSync("./dist/cli-bin.js", emitFile.getText());
