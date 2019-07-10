import { PropertySignature } from "ts-morph";
import { BabelAnalyzerContext } from "./BabelAnalyzerContext";

export class BabelNodeProperty {
    constructor(private readonly context: BabelAnalyzerContext, private readonly declaration: PropertySignature) {
    }

    getName() {
        return this.declaration.getName();
    }

    isReferenced() {
        const references = this.declaration.findReferences();
        const mainSrcDir = this.context.getProject().getDirectoryOrThrow("src");

        return references.some(s => s.getReferences().some(reference => mainSrcDir.isAncestorOf(reference.getSourceFile())));
    }
}
