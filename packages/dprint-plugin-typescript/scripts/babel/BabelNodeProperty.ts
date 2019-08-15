import { Node, PropertySignature, SyntaxKind, StringLiteral, LiteralTypeNode } from "ts-morph";
import { BabelAnalyzerContext } from "./BabelAnalyzerContext";

export class BabelNodeProperty {
    constructor(private readonly context: BabelAnalyzerContext, private readonly declaration: PropertySignature) {
    }

    getName() {
        return this.declaration.getName();
    }

    getIgnoredReasonMessage() {
        const nodes = this.findReferencesAsNodes();
        const message = getMessage();

        if (message != null && nodes.length > 1) {
            // todo: add the property and parent name to this message
            console.warn("A node was marked to be ignored, but it is used in more than one place.");
        }

        return message;

        function getMessage() {
            for (const node of nodes) {
                // can't wait for conditional property access...
                const propRef = node.getParentIfKind(SyntaxKind.QualifiedName);
                const typeQuery = propRef && propRef.getParentIfKind(SyntaxKind.TypeQuery);
                const typeReference = typeQuery && typeQuery.getParentIfKind(SyntaxKind.TypeReference);
                if (typeReference && typeReference.getTypeName().getText() === "AnalysisMarkIgnored") {
                    // assume this is done correctly...
                    const typeLiteral = typeReference.getTypeArguments()[1] as LiteralTypeNode;
                    const stringLiteral = typeLiteral.getLiteral() as StringLiteral;
                    return stringLiteral.getLiteralValue();
                }
            }

            return undefined;
        }
    }

    isReferenced() {
        return this.findReferencesAsNodes().length > 0;
    }

    private referencesAsNodes: Node[] | undefined;
    private findReferencesAsNodes() {
        if (this.referencesAsNodes == null) {
            const mainSrcDir = this.context.getProject().getDirectoryOrThrow("src");
            this.referencesAsNodes = this.declaration.findReferencesAsNodes()
                .filter(node => mainSrcDir.isAncestorOf(node.getSourceFile()));
        }
        return this.referencesAsNodes;
    }
}
