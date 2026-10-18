export async function getPluginDefaultConfig(configSchemaUrl: string, signal: AbortSignal) {
  if (configSchemaUrl == null) {
    return "{\n}\n";
  }

  try {
    const response = await fetch(configSchemaUrl, {
      signal,
    });
    const json = await response.json();
    let text = "{";
    let wroteProperty = false;

    for (const propertyName of Object.keys(json.properties)) {
      if (propertyName === "$schema" || propertyName === "deno" || propertyName === "locked") {
        continue;
      }
      const property = json.properties[propertyName];
      const derivedPropName = property["$ref"]?.replace("#/definitions/", "");

      const lastSegment = propertyName.split(".").pop()!;
      const astSpecific = (derivedPropName !== propertyName && derivedPropName in json.properties)
        || (lastSegment !== propertyName && lastSegment in json.properties);
      if (astSpecific) {
        continue;
      }

      let defaultValue: string | boolean | number | undefined;

      if (derivedPropName) {
        const definition = json.definitions[derivedPropName];
        if (definition != null) {
          defaultValue = definition?.default;
        }
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
            text += `${defaultValue?.toString() ?? "null"}`;
          }
        }

        wroteProperty = true;
      }
    }

    text += "\n}\n";

    return text;
  } catch (err: any) {
    if (signal.aborted) {
      throw err;
    }
    return `{\n  // error resolving schema: ${err?.toString()}\n}\n`;
  }
}
