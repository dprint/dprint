/// <reference lib="deno.ns" />

import { deepStrictEqual } from "node:assert/strict";
import { getPluginSchemaDefaults } from "./getPluginSchemaDefaults.ts";

function getDefaults(schema: unknown) {
  const url = `data:application/json,${encodeURIComponent(JSON.stringify(schema))}`;
  return getPluginSchemaDefaults(url, new AbortController().signal);
}

Deno.test("gets the playground's sample from the schema", async () => {
  const defaults = await getDefaults({
    properties: { lineWidth: { type: "integer", default: 120 } },
    "x-playground": { extension: "tsx", input: ["const  a=1", "const b  =2"] },
  });

  deepStrictEqual(defaults.sample, { text: "const  a=1\nconst b  =2\n", extension: "tsx" });
  deepStrictEqual(defaults.configText, "{\n  \"lineWidth\": 80\n}\n");
});

Deno.test("ignores a sample that isn't valid", async () => {
  for (const playground of [undefined, { input: "a" }, { input: [1] }, { input: ["a".repeat(20_001)] }]) {
    const defaults = await getDefaults({ properties: {}, "x-playground": playground });
    deepStrictEqual(defaults.sample, undefined);
  }
  // an extension that isn't a string is left for the playground to choose
  const defaults = await getDefaults({ properties: {}, "x-playground": { extension: 5, input: ["a"] } });
  deepStrictEqual(defaults.sample, { text: "a\n", extension: undefined });
});
