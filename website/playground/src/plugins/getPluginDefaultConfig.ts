import { PluginInfo } from "./getPluginInfo";

export async function getPluginDefaultConfig(plugin: PluginInfo) {
    const response = await fetch(plugin.configSchemaUrl);
    const json = await response.json();
    let text = "{";
    let wroteProperty = false;

    for (const propertyName of Object.keys(json.properties)) {
        if (propertyName === "$schema" || propertyName === "deno" || propertyName === "locked") {
            continue;
        }
        const property = json.properties[propertyName];
        let defaultValue: string | boolean | number | undefined;

        if (property["$ref"]) {
            defaultValue = json.definitions[propertyName]?.default;
        } else {
            defaultValue = property.default;
        }

        if (defaultValue != null) {
            if (wroteProperty) {
                text += ",\n";
            } else {
                text += "\n";
            }

            text += `  "${propertyName}": `;
            if (typeof defaultValue === "string") {
                text += `"${defaultValue}"`;
            } else {
                if (propertyName === "lineWidth") {
                    text += "80";
                } else {
                    text += `${defaultValue.toString()}`;
                }
            }

            wroteProperty = true;
        }
    }

    text += "\n}\n";

    return text;
}
