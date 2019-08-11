import { Project, SyntaxKind, TypeGuards } from "ts-morph";
import { DPrintAnalyzerContext } from "./DPrintAnalyzerContext";

export class DPrintAnalyzer {
    private readonly context: DPrintAnalyzerContext;

    constructor(private readonly project: Project) {
        this.context = new DPrintAnalyzerContext(project);
    }

    getParserParseObjKeys() {
        const ole = this.getParseObjectInitializer();
        return ole.getProperties()
            .filter(TypeGuards.isPropertyAssignment)
            .map(p => p.getName().slice(1, -1));
    }

    getIgnoredUnknownNodeNames() {
        return this.getPropertyNamesWithInitializerText("parseUnknownNode");
    }

    getIgnoredFlowNodeNames() {
        return this.getPropertyNamesWithInitializerText("parseNotSupportedFlowNode");
    }

    private getPropertyNamesWithInitializerText(initializerText: string) {
        const ole = this.getParseObjectInitializer();
        return ole.getProperties()
            .filter(TypeGuards.isPropertyAssignment)
            .filter(p => p.getInitializerOrThrow().getText() === initializerText)
            .map(p => p.getName().slice(1, -1));
    }

    private getParseObjectInitializer() {
        const parserFile = this.project.getSourceFileOrThrow("src/parsing/parser.ts");
        const parseObj = parserFile.getVariableDeclarationOrThrow("parseObj");
        return parseObj.getInitializerIfKindOrThrow(SyntaxKind.ObjectLiteralExpression);
    }
}
