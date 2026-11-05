import { deepStrictEqual, strictEqual } from "node:assert/strict";
import { getPlaceholderReplacement, getPluginUrlKey, type LatestPluginRelease, parseVersionPlaceholder, readPluginInfo } from "./plugin_versions.ts";

const info = readPluginInfo({
  schemaVersion: 4,
  latest: [
    { url: "https://plugins.dprint.dev/typescript-0.97.1.wasm", npm: { name: "@dprint/typescript", version: "0.97.1" } },
    { url: "https://plugins.dprint.dev/jolars/panache-v1.4.1.wasm" },
    { url: "https://plugins.dprint.dev/drluckyspin/swift-v0.1.0.json" },
  ],
});

Deno.test("parses placeholders", () => {
  deepStrictEqual(parseVersionPlaceholder(`"npm:@dprint/typescript@x.x.x"`), {
    kind: "npm",
    name: "@dprint/typescript",
  });
  deepStrictEqual(parseVersionPlaceholder(`"https://plugins.dprint.dev/jolars/panache-x.x.x.wasm"`), {
    kind: "url",
    key: "https://plugins.dprint.dev/jolars/panache-x.x.x.wasm",
    plugin: "jolars/panache",
    hasChecksum: false,
  });
  deepStrictEqual(parseVersionPlaceholder(`"https://plugins.dprint.dev/drluckyspin/swift-vx.x.x.json@<checksum>"`), {
    kind: "url",
    key: "https://plugins.dprint.dev/drluckyspin/swift-x.x.x.json",
    plugin: "drluckyspin/swift",
    hasChecksum: true,
  });
  strictEqual(parseVersionPlaceholder(`"npm:@dprint/typescript@0.97.1"`), undefined);
  strictEqual(parseVersionPlaceholder(`"https://example.com/plugin-x.x.x.wasm"`), undefined);
  strictEqual(parseVersionPlaceholder(`"lineWidth"`), undefined);
});

Deno.test("matches urls regardless of a leading v", () => {
  strictEqual(
    getPluginUrlKey("https://plugins.dprint.dev/jolars/panache-v1.4.1.wasm"),
    getPluginUrlKey("https://plugins.dprint.dev/jolars/panache-x.x.x.wasm"),
  );
  strictEqual(getPluginUrlKey("https://plugins.dprint.dev/typescript-0.97.1.wasm"), "https://plugins.dprint.dev/typescript-x.x.x.wasm");
});

Deno.test("replaces placeholders with the latest versions", async () => {
  const latestReleases: Record<string, LatestPluginRelease> = {
    "drluckyspin/swift": { url: "https://plugins.dprint.dev/drluckyspin/swift-v0.1.0.json", checksum: "abc" },
    "jolars/fatou": { url: "https://plugins.dprint.dev/jolars/fatou-v0.7.7.wasm", checksum: undefined },
  };
  const requested: string[] = [];
  const replace = (text: string) =>
    getPlaceholderReplacement(parseVersionPlaceholder(text)!, info, (plugin) => {
      requested.push(plugin);
      return Promise.resolve(latestReleases[plugin]);
    });

  strictEqual(await replace(`"npm:@dprint/typescript@x.x.x"`), `"npm:@dprint/typescript@0.97.1"`);
  strictEqual(await replace(`"npm:@dprint/unknown@x.x.x"`), undefined);
  strictEqual(
    await replace(`"https://plugins.dprint.dev/jolars/panache-x.x.x.wasm"`),
    `"https://plugins.dprint.dev/jolars/panache-v1.4.1.wasm"`,
  );
  // not in the info.json
  strictEqual(
    await replace(`"https://plugins.dprint.dev/jolars/fatou-vx.x.x.wasm"`),
    `"https://plugins.dprint.dev/jolars/fatou-v0.7.7.wasm"`,
  );
  // the checksum is only in the latest.json
  strictEqual(
    await replace(`"https://plugins.dprint.dev/drluckyspin/swift-vx.x.x.json@<checksum>"`),
    `"https://plugins.dprint.dev/drluckyspin/swift-v0.1.0.json@abc"`,
  );
  strictEqual(await replace(`"https://plugins.dprint.dev/jolars/missing-x.x.x.wasm"`), undefined);
  deepStrictEqual(requested, ["jolars/fatou", "drluckyspin/swift", "jolars/missing"]);
});
