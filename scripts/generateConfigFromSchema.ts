import { Project, PropertySignatureStructure, OptionalKind, NewLineKind } from "ts-morph";
import Schema from "../src/configuration/dprint.schema.json";

const project = new Project({ manipulationSettings: { newLineKind: NewLineKind.CarriageReturnLineFeed } });
const file = project.addExistingSourceFile("src/configuration/Configuration.ts");
const config = file.getInterfaceOrThrow("Configuration");

interface SchemaProperty {
    type: string;
    description?: string;
    default: boolean | string;
    oneOf?: { const: string; description: string; }[];
}

// remove existing properties
config.getProperties().forEach(p => p.remove());

// add the new ones in
const properties: OptionalKind<PropertySignatureStructure>[] = [];
for (const propName in Schema.properties) {
    const prop = (Schema.properties as any)[propName] as SchemaProperty;
    properties.push({
        name: propName.indexOf(".") >= 0 ? `"${propName}"` : propName,
        hasQuestionToken: true,
        type: getType(propName, prop),
        docs: getDocs(prop)
    });
}

config.addProperties(properties);

file.saveSync();

function getDocs(prop: SchemaProperty) {
    let result: string | undefined;
    if (prop.description != null)
        result = prop.description;

    if (prop.default != null) {
        if (result == null)
            result = "";
        else
            result += "\r\n";

        result += "@default " + getDefault()
    }
    return result == null ? undefined : [result];

    function getDefault() {
        return prop.type === "string" ? `"${prop.default}"` : prop.default;
    }
}

function getType(propName: string, prop: SchemaProperty) {
    if (prop.type !== "string")
        return prop.type;
    const oneOf = prop.oneOf;
    if (oneOf == null)
        throw new Error(`Expected a oneOf property for ${propName}.`);

    return oneOf.map(item => `"${item.const}"`).join(" | ");
}
