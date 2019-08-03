import { Project, PropertySignatureStructure, OptionalKind, NewLineKind, SyntaxKind, PropertyAssignmentStructure, ObjectLiteralExpression,
    Writers } from "ts-morph";
import { parseJsonSchemaProperties, SchemaProperty } from "./parseJsonSchemaProperties";

const project = new Project({ manipulationSettings: { newLineKind: NewLineKind.CarriageReturnLineFeed } });

// add the new ones in
const jsonSchemaProperties = Array.from(parseJsonSchemaProperties());
const properties: OptionalKind<PropertySignatureStructure>[] = [];
const defaultValueProperties: OptionalKind<PropertyAssignmentStructure>[] = [];

for (const prop of jsonSchemaProperties) {
    const sanitizedPropName = prop.name.indexOf(".") >= 0 ? `"${prop.name}"` : prop.name;
    properties.push({
        name: sanitizedPropName,
        hasQuestionToken: true,
        type: getType(prop.name, prop),
        docs: getDocs(prop)
    });

    if (isAllowedDefaultValueProperty(prop, sanitizedPropName)) {
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
configurationFile.save();

// set the default values object
const resolveConfigurationFile = project.addExistingSourceFile("src/configuration/resolveConfiguration.ts");
const defaultValuesObj = resolveConfigurationFile.getVariableDeclarationOrThrow("defaultValues")
    .getInitializerIfKindOrThrow(SyntaxKind.AsExpression)
    .getExpression() as ObjectLiteralExpression;

defaultValuesObj.getProperties().forEach(p => p.remove());
defaultValuesObj.addPropertyAssignments(defaultValueProperties);
resolveConfigurationFile.save();

// update the projectTypeInfo object
const projectTypeProperty = jsonSchemaProperties.find(p => p.name === "projectType")!;
const getMissingProjectTypeDiagnosticFile = project.addExistingSourceFile("src/configuration/getMissingProjectTypeDiagnostic.ts");
const projectTypeInfoObj = getMissingProjectTypeDiagnosticFile.getVariableDeclarationOrThrow("projectTypeInfo")
    .getInitializerIfKindOrThrow(SyntaxKind.ObjectLiteralExpression);
projectTypeInfoObj.getProperties().forEach(p => p.remove());
projectTypeInfoObj.addPropertyAssignments([{
    name: "values",
    initializer: writer => {
        writer.write("[").newLine();
        writer.indentBlock(() => {
            const oneOf = projectTypeProperty.oneOf!;
            for (let i = 0; i < oneOf.length; i++) {
                writer.writeLine("{");
                writer.indentBlock(() => {
                    writer.write("name: ").quote(oneOf[i].const).write(",").newLine();
                    writer.write("description: ").quote(oneOf[i].description).newLine();
                });
                writer.write("}");

                if (i < oneOf.length - 1)
                    writer.write(",");

                writer.newLine();
            }
        });
        writer.write("]");
    }
}]);
getMissingProjectTypeDiagnosticFile.save();

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
            result += `@value "${value.const}" - ${value.description}`;
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

function isAllowedDefaultValueProperty(prop: SchemaProperty, sanitizedPropName: string) {
    if (prop.name === "enumDeclaration.memberSpacing")
        return true;

    return prop.default != null && sanitizedPropName === prop.name;
}
