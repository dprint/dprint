import { InterfaceDeclaration } from "ts-morph";
import { BabelAnalyzerContext } from "./BabelAnalyzerContext";

export class BabelNode {
    constructor(private readonly context: BabelAnalyzerContext, private readonly declaration: InterfaceDeclaration) {
    }

    getName() {
        return this.declaration.getName();
    }

    getType() {
        return this.declaration.getPropertyOrThrow("type").getTypeNodeOrThrow().getText().slice(1, -1);
    }

    getProperties() {
        return this.declaration.getProperties().map(p => this.context.getNodeProperty(p));
    }

    isReferenced() {
        const references = this.declaration.findReferences();
        const mainSrcDir = this.context.getProject().getDirectoryOrThrow("src");

        return references.some(s => s.getReferences().some(reference => mainSrcDir.isAncestorOf(reference.getSourceFile())));
    }
}
