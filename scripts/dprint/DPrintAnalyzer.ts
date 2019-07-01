import { Project, SyntaxKind, TypeGuards } from "ts-morph";
import { DPrintAnalyzerContext } from "./DPrintAnalyzerContext";

export class DPrintAnalyzer {
    private readonly context: DPrintAnalyzerContext;

    constructor(private readonly project: Project) {
        this.context = new DPrintAnalyzerContext(project);
    }

    getParserParseObjKeys() {
        const parserFile = this.project.getSourceFileOrThrow("src/parsing/parser.ts");
        const parseObj = parserFile.getVariableDeclarationOrThrow("parseObj");
        const ole = parseObj.getInitializerIfKindOrThrow(SyntaxKind.ObjectLiteralExpression);
        return ole.getProperties()
            .filter(TypeGuards.isPropertyAssignment)
            .map(p => p.getName().slice(1, -1));
    }
}