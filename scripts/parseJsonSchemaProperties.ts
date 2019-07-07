import Schema from "../src/configuration/dprint.schema.json";

export interface SchemaProperty {
    name: string;
    type: string;
    description?: string;
    default: boolean | string | undefined;
    oneOf?: { const: string; description: string; }[];
}

interface SchemaPointer {
    "$ref": string;
}

export function* parseJsonSchemaProperties(): Iterable<SchemaProperty> {
    for (const propName in Schema.properties) {
        const prop = (Schema.properties as any)[propName] as SchemaProperty | SchemaPointer;
        const ref = (prop as SchemaPointer).$ref;
        if (ref != null) {
            yield {
                ...(Schema.definitions as any)[ref.replace("#/definitions/", "")],
                name: propName
            } as SchemaProperty;
        }
        else {
            yield {
                ...(Schema.properties as any)[propName],
                name: propName
            } as SchemaProperty;
        }
    }
}
