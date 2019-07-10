import { Project, PropertySignatureStructure, OptionalKind, NewLineKind, SyntaxKind, PropertyAssignmentStructure, ObjectLiteralExpression } from "ts-morph";
import { parseJsonSchemaProperties, SchemaProperty } from "./parseJsonSchemaProperties";

const project = new Project({ manipulationSettings: { newLineKind: NewLineKind.CarriageReturnLineFeed } });

// add the new ones in
const properties: OptionalKind<PropertySignatureStructure>[] = [];
const defaultValueProperties: OptionalKind<PropertyAssignmentStructure>[] = [];
const jsonSchema = parseJsonSchemaProperties();
for (const prop of parseJsonSchemaProperties()) {
    const sanitizedPropName = prop.name.indexOf(".") >= 0 ? `"${prop.name}"` : prop.name;
    properties.push({
        name: sanitizedPropName,
        hasQuestionToken: true,
        type: getType(prop.name, prop),
        docs: getDocs(prop)
    });

    if (prop.default != null && sanitizedPropName === prop.name) {
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
        appendNewLine();
        result += "@default " + getDefault(prop);
    }
    if (prop.oneOf != null) {
        for (const value of prop.oneOf) {
            appendNewLine();
            result += `@value "${value.const}" - ${value.description}`
        }
    }
    return result == null ? undefined : [result];

    function appendNewLine() {
        if (result == null)
            result = "";
        else
            result += "\r\n";
    }
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
