import { Project, TypeGuards } from "ts-morph";
import { BabelNode } from "./BabelNode";
import { BabelAnalyzerContext } from "./BabelAnalyzerContext";

export class BabelAnalyzer {
    private readonly context: BabelAnalyzerContext;

    constructor(private readonly project: Project) {
        this.context = new BabelAnalyzerContext(project);
    }

    getNodes() {
        const sourceFile = this.project.getSourceFileOrThrow("parser.ts")
            .getImportDeclarationOrThrow("@babel/types")
            .getModuleSpecifierSourceFileOrThrow();

        const nodeTypeAlias = sourceFile.getTypeAliasOrThrow("Node");
        const result: BabelNode[] = [];

        for (const type of nodeTypeAlias.getType().getUnionTypes()) {
            const declarations = type.getSymbolOrThrow().getDeclarations();
            if (declarations.length !== 1)
                throw new Error("Unexpected node that didn't have a single declaration.");

            const declaration = declarations[0];
            if (!TypeGuards.isInterfaceDeclaration(declaration))
                throw new Error("Unexpected node that wasn't an interface.");

            result.push(this.context.getNode(declaration));
        }

        return result;
    }
}
