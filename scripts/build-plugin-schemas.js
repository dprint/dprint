const fs = require("fs");
const { Project, Node } = require("ts-morph");
const CodeBlockWriter = require("code-block-writer").default;

writeFile("dprint-plugin-typescript", "TypeScriptConfiguration", "typescript-v0");
writeFile("dprint-plugin-jsonc", "JsoncConfiguration", "json-v0");

function writeFile(pluginName, interfaceName, schemaFileName) {
    const text = buildText(pluginName, interfaceName, schemaFileName);
    // This is hard coded for my system in the interest of saving time. This should be improved in the future...
    fs.writeFileSync(`V:/dprint-plugins/schemas/${schemaFileName}.json`, text, { encoding: "utf8" });
}

function buildText(pluginName, interfaceName, schemaFileName) {
    const project = new Project({ tsConfigFilePath: `V:/dprint-node/packages/${pluginName}/tsconfig.json` });
    const configFile = project.getSourceFileOrThrow("Configuration.ts");
    const configClass = configFile.getInterfaceOrThrow(interfaceName);
    const propDefinitions = configClass.getProperties().map(getPropertyDefinition);

    const writer = new CodeBlockWriter({ indentNumberOfSpaces: 2 });
    writer.block(() => {
        writer.quote("$schema").write(": ").quote("http://json-schema.org/draft-07/schema#").write(",").newLine();
        writer.quote("$id").write(": ").quote(`https://plugins.dprint.dev/schemas/${schemaFileName}.json`).write(",").newLine();
        writer.quote("type").write(": ").quote("object").write(",").newLine();
        writer.quote("definitions").write(": ").inlineBlock(() => {
            for (const [index, prop] of propDefinitions.filter(p => p.type === "union" || p.type === "boolean").entries()) {
                if (index > 0) {
                    writer.write(",").newLine();
                }
                writer.quote(prop.name).write(": ").inlineBlock(() => {
                    writer.quote("description").write(": ").quote(santizeDescription(prop.description)).write(",").newLine();
                    writer.quote("type").write(": ").quote(prop.type === "union" ? "string" : "boolean").write(",").newLine();
                    if (prop.defaultValue != null) {
                        writer.quote("default").write(": ").write(prop.defaultValue.toString()).write(",").newLine();
                    }
                    writer.quote("oneOf").write(": [");
                    for (const [index, value] of prop.values.entries()) {
                        writer.conditionalWrite(index > 0, ", ");
                        writer.write("{").newLine().indent(() => {
                            writer.quote("const").write(": ");
                            if (prop.type === "boolean") {
                                writer.write(value.name);
                            } else {
                                writer.quote(value.name);
                            }
                            writer.write(",").newLine();
                            writer.quote("description").write(": ").quote(santizeDescription(value.description)).newLine();
                        }).write("}");
                    }
                    writer.write("]");
                });
            }
            writer.newLine();
        }).write(",").newLine();
        writer.quote("properties").write(": ").block(() => {
            writer.quote("$schema").write(": ").inlineBlock(() => {
                writer.quote("description").write(": ").quote("The JSON schema reference.").write(",").newLine();
                writer.quote("type").write(": ").quote("string");
            }).write(",").newLine();
            writer.quote("locked").write(": ").inlineBlock(() => {
                writer.quote("description").write(": ").quote("Whether the configuration is allowed to be overriden or extended.").write(",").newLine();
                writer.quote("type").write(": ").quote("boolean");
            }).write(",").newLine();

            for (const [index, prop] of propDefinitions.entries()) {
                if (index > 0) {
                    writer.write(",").newLine();
                }
                const name = prop.name;
                writer.quote(name).write(": ").inlineBlock(() => {
                    if (prop.type === "union" || prop.type === "boolean") {
                        writer.quote("$ref").write(": ").quote(`#/definitions/${name}`);
                    } else if (prop.type === "string" || prop.type === "number") {
                        writer.quote("description").write(": ").quote(santizeDescription(prop.description)).write(",").newLine();
                        if (prop.defaultValue != null) {
                            writer.quote("default").write(": ").write(prop.defaultValue.toString()).write(",").newLine();
                        }
                        writer.quote("type").write(": ").quote(prop.type);
                    } else if (prop.type === "ref") {
                        writer.quote("$ref").write(": ").quote(`#/definitions/${prop.reference}`);
                    } else {
                        throw new Error("Not handled. " + prop.type);
                    }
                });
            }
        });
    });

    return writer.toString();
}

/** @param {import("ts-morph").PropertySignature} [prop] */
function getPropertyDefinition(prop) {
    const name = prop.getName().replace(/"/g, "");
    const typeNode = prop.getTypeNodeOrThrow();
    const jsDoc = prop.getJsDocs()[0];
    const description = jsDoc && jsDoc.getDescription().trim();
    let values;
    let type;
    let reference;

    if (Node.isUnionTypeNode(typeNode)) {
        values = getUnionValues(prop, typeNode);
        type = "union";
    } else if (Node.isBooleanKeyword(typeNode)) {
        values = getBoolValues(prop);
        type = "boolean";
    } else if (Node.isStringKeyword(typeNode)) {
        type = "string";
    } else if (Node.isNumberKeyword(typeNode)) {
        type = "number";
    } else if (Node.isIndexedAccessTypeNode(typeNode)) {
        type = "ref";
        const indexTypeNode = typeNode.getIndexTypeNode();
        if (Node.isLiteralTypeNode(indexTypeNode)) {
            const literal = indexTypeNode.getLiteral();
            if (Node.isStringLiteral(literal)) {
                reference = literal.getLiteralValue();
            } else {
                throw new Error("Unhandled literal value kind.");
            }
        } else {
            throw new Error("Not handled index type node.");
        }
    } else {
        throw new Error("Not handled: " + typeNode.getKindName());
    }

    return {
        name,
        type,
        description,
        values,
        reference,
        defaultValue: getDefaultValue(prop),
    };
}

/**
 * @param {import("ts-morph").PropertySignature} [prop]
 * @param {import("ts-morph").TypeNode} [typeNode]
 */
function getUnionValues(prop, typeNode) {
    const jsDoc = prop.getJsDocs()[0];
    const tags = jsDoc && jsDoc.getTags() || [];
    const items = [];
    for (const itemTypeNode of typeNode.getTypeNodes()) {
        if (Node.isLiteralTypeNode(itemTypeNode)) {
            const literal = itemTypeNode.getLiteral();
            if (Node.isStringLiteral(literal)) {
                const name = literal.getLiteralValue();
                const tag = tags.find(t => t.getTagName() === "value" && t.getComment().startsWith(`"${name}"`));

                items.push({
                    name,
                    description: tag.getComment().replace(`"${name}" - `, ""),
                });
            } else {
                throw new Error("Not expected.");
            }
        } else {
            throw new Error("Not expected.");
        }
    }
    return items;
}

/**
 * @param {import("ts-morph").PropertySignature} [prop]
 */
function getBoolValues(prop) {
    const jsDoc = prop.getJsDocs()[0];
    const tags = jsDoc && jsDoc.getTags() || [];
    const items = [];

    items.push(getForValue(true));
    items.push(getForValue(false));

    return items;

    /** @param {boolean} [value] */
    function getForValue(value) {
        const tag = tags.find(t => t.getTagName() === "value" && t.getComment().startsWith(value.toString()));

        return {
            name: value.toString(),
            description: tag && tag.getComment().replace(`${value} - `, "").trim(),
        };
    }
}

/**
 * @param {import("ts-morph").PropertySignature} [prop]
 */
function getDefaultValue(prop) {
    const jsDoc = prop.getJsDocs()[0];
    const tags = jsDoc && jsDoc.getTags() || [];

    const defaultTag = tags.find(t => t.getTagName() === "default");
    let result = defaultTag && defaultTag.getComment().trim();
    if (result == null) {
        return null;
    }
    if (result.startsWith("`") && result.endsWith("`")) {
        result = result.substring(1, result.length - 1);
    }

    if (result === "true") {
        return true;
    } else if (result === "false") {
        return false;
    } else if (result.startsWith("\"") && result.endsWith("\"")) {
        return result;
    } else if (!isNaN(parseInt(result, 10))) {
        return parseInt(result, 10);
    } else {
        throw new Error("Not handled value: " + result);
    }
}

/** @param {string} [text] */
function santizeDescription(text) {
    text = (text || "").trim().split(/\r?\n/g).join(" ").split(/\r/).join(" ");
    text = text.replace(/\\`/g, "`");
    return text;
}
