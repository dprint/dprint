import { Project, PropertySignatureStructure, OptionalKind, NewLineKind, SyntaxKind, PropertyAssignmentStructure, ObjectLiteralExpression } from "ts-morph";
import Schema from "../src/configuration/dprint.schema.json";

const project = new Project({ manipulationSettings: { newLineKind: NewLineKind.CarriageReturnLineFeed } });

interface SchemaProperty {
    type: string;
    description?: string;
    default: boolean | string | undefined;
    oneOf?: { const: string; description: string; }[];
}

// add the new ones in
const properties: OptionalKind<PropertySignatureStructure>[] = [];
const defaultValueProperties: OptionalKind<PropertyAssignmentStructure>[] = [];
for (const propName in Schema.properties) {
    const prop = (Schema.properties as any)[propName] as SchemaProperty;
    const sanitizedPropName = propName.indexOf(".") >= 0 ? `"${propName}"` : propName;
    properties.push({
        name: sanitizedPropName,
        hasQuestionToken: true,
        type: getType(propName, prop),
        docs: getDocs(prop)
    });

    if (prop.default != null) {
        defaultValueProperties.push({
            name: sanitizedPropName,
            initializer: getDefault(prop)
        });
    }
}

// update the configuration file
const configurationFile = project.addExistingSourceFile("src/configuration/Configuration.ts");
const configClass = configurationFile.getInterfaceOrThrow("Configuration");
configClass.getProperties().forEach(p => p.remove());
configClass.addProperties(properties);
configurationFile.saveSync();

// set the default values object
const resolveConfigurationFile = project.addExistingSourceFile("src/configuration/resolveConfiguration.ts");
const defaultValuesObj = resolveConfigurationFile.getVariableDeclarationOrThrow("defaultValues")
    .getInitializerIfKindOrThrow(SyntaxKind.AsExpression)
    .getExpression() as ObjectLiteralExpression;

defaultValuesObj.getProperties().forEach(p => p.remove());
defaultValuesObj.addPropertyAssignments(defaultValueProperties);
resolveConfigurationFile.saveSync();

function getDocs(prop: SchemaProperty) {
    let result: string | undefined;
    if (prop.description != null)
        result = prop.description;

    if (prop.default != null) {
        if (result == null)
            result = "";
        else
            result += "\r\n";

        result += "@default " + getDefault(prop)
    }
    return result == null ? undefined : [result];
}

function getDefault(prop: SchemaProperty) {
    return prop.type === "string" ? `"${prop.default}"` : prop.default != null ? prop.default.toString() : "undefined";
}

function getType(propName: string, prop: SchemaProperty) {
    if (prop.type !== "string")
        return prop.type;
    const oneOf = prop.oneOf;
    if (oneOf == null)
        throw new Error(`Expected a oneOf property for ${propName}.`);

    return oneOf.map(item => `"${item.const}"`).join(" | ");
}
