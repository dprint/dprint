export async function getPluginDefaultConfig(configSchemaUrl: string, signal: AbortSignal) {
  if (configSchemaUrl == null) {
    return "{\n}\n";
  }

  try {
    const response = await fetch(configSchemaUrl, {
      signal,
    });
    const json = await response.json();

    const text = JSON.stringify(json.properties, (key, value) => {
      if (key === "$schema" || key === "deno" || key === "locked") {
        return;
      }
      const derivedPropName = value["$ref"]?.replace("#/definitions/", "");

      const lastSegment = key.split(".").pop()!;
      const astSpecific = (derivedPropName !== key && Object.hasOwn(json.properties, derivedPropName))
        || (lastSegment !== key && Object.hasOwn(json.properties, lastSegment));
      if (astSpecific) {
        return;
      }

      let defaultValue: string | boolean | number | undefined;

      if (derivedPropName) {
        const definition = json.definitions[derivedPropName];
        if (definition != null) {
          defaultValue = definition?.default;
        }
      } else if (Object.hasOwn(json.properties, key)) {
        defaultValue = value.default;
      } else {
        defaultValue = value;
      }

      return key === "lineWidth" ? 80 : defaultValue;
    }, 2);

    return text + "\n";
  } catch (err: any) {
    if (signal.aborted) {
      throw err;
    }
    return `{\n  // error resolving schema: ${err?.toString()}\n}\n`;
  }
}
