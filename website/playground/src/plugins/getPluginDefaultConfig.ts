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

      const lastSegment = propertyName.split(".").pop()!;
      const astSpecific = lastSegment !== propertyName && json.properties[lastSegment] != null;
      if (astSpecific) {
        continue;
      }

      const property = json.properties[propertyName];
      let defaultValue: string | boolean | number | undefined;

      if (property["$ref"]) {
        const derivedPropName = property["$ref"].replace("#/definitions/", "");
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
