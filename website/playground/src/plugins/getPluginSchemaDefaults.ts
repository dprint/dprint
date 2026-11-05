/** What the playground starts with for a plugin, from its config schema. */
export interface PluginSchemaDefaults {
  configText: string;
  /** The code the playground starts with, from the schema's `x-playground`. */
  sample: PlaygroundSample | undefined;
}

export interface PlaygroundSample {
  text: string;
  /** The file extension of the code, or undefined to use the plugin's first one. */
  extension: string | undefined;
}

// the most characters of a sample, which is what dprint/automation's publish-schema allows
const MAX_SAMPLE_LENGTH = 20_000;

export async function getPluginSchemaDefaults(configSchemaUrl: string, signal: AbortSignal): Promise<PluginSchemaDefaults> {
  if (configSchemaUrl == null) {
    return { configText: "{\n}\n", sample: undefined };
  }

  try {
    const response = await fetch(configSchemaUrl, {
      signal,
    });
    const json = await response.json();
    return { configText: getDefaultConfigText(json), sample: getPlaygroundSample(json) };
  } catch (err: any) {
    if (signal.aborted) {
      throw err;
    }
    return { configText: `{\n  // error resolving schema: ${err?.toString()}\n}\n`, sample: undefined };
  }
}

function getDefaultConfigText(json: any) {
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
}

// the code is only ever shown in the editor as text, but it's still checked
// since the schema may be any plugin's
function getPlaygroundSample(json: any): PlaygroundSample | undefined {
  const playground = json?.["x-playground"];
  const input = playground?.input;
  if (!Array.isArray(input) || !input.every((line) => typeof line === "string")) {
    return undefined;
  }
  const text = input.join("\n") + "\n";
  if (text.length > MAX_SAMPLE_LENGTH) {
    return undefined;
  }
  return { text, extension: typeof playground.extension === "string" ? playground.extension : undefined };
}
