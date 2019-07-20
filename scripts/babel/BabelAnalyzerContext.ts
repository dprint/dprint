import { Project, InterfaceDeclaration, PropertySignature } from "ts-morph";
import { BabelNode } from "./BabelNode";
import { BabelNodeProperty } from "./BabelNodeProperty";

export class BabelAnalyzerContext {
    private readonly nodes = new Map<InterfaceDeclaration, BabelNode>();
    private readonly nodeProperties = new Map<PropertySignature, BabelNodeProperty>();

    constructor(private readonly project: Project) {
    }

    getProject() {
        return this.project;
    }

    getNode(interfaceDec: InterfaceDeclaration) {
        let result = this.nodes.get(interfaceDec);
        if (result == null) {
            result = new BabelNode(this, interfaceDec);
            this.nodes.set(interfaceDec, result);
        }
        return result;
    }

    getNodeProperty(property: PropertySignature) {
        let result = this.nodeProperties.get(property);
        if (result == null) {
            result = new BabelNodeProperty(this, property);
            this.nodeProperties.set(property, result);
        }
        return result;
    }
}
