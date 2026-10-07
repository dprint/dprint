#!/usr/bin/env -S deno run -A
// Starts up the latest version of every plugin listed on plugins.dprint.dev
// with the provided dprint binary. This catches changes to dprint that break
// published plugins (ex. https://github.com/dprint/dprint/issues/1306).
//
// Usage: ./test_known_plugins.ts <path-to-dprint-binary>
import $ from "jsr:@david/dax@0.45.0";

interface PluginInfo {
  name: string;
  configKey: string;
  url: string;
}

type PluginResult = { kind: "ok" } | { kind: "skipped"; reason: string } | { kind: "failed"; output: string };

const INFO_URL = "https://plugins.dprint.dev/info.json";
const CONCURRENCY = 4;
// configuration for the plugins that have diagnostics without any
const PLUGIN_CONFIGS: Record<string, unknown> = {
  "dprint-plugin-exec": { commands: [] },
};

if (Deno.args.length !== 1) {
  console.error("Usage: test_known_plugins.ts <path-to-dprint-binary>");
  Deno.exit(1);
}
const dprint = $.path(Deno.args[0]).resolve();
const plugins: PluginInfo[] = (await $.request(INFO_URL).json()).latest;
if (plugins.length === 0) {
  throw new Error(`No plugins found at ${INFO_URL}`);
}

const tempDir = $.path(await Deno.makeTempDir({ prefix: "dprint_known_plugins_" }));
const failures: { plugin: PluginInfo; output: string }[] = [];
let skippedCount = 0;
try {
  const pending = [...plugins];
  await Promise.all(Array.from({ length: CONCURRENCY }, async () => {
    let plugin: PluginInfo | undefined;
    while ((plugin = pending.shift()) != null) {
      const result = await testPlugin(plugin, tempDir.join(String(plugins.indexOf(plugin))));
      switch (result.kind) {
        case "ok":
          $.logStep("Passed", plugin.url);
          break;
        case "skipped":
          $.logWarn("Skipped", `${plugin.url} (${result.reason})`);
          skippedCount++;
          break;
        case "failed":
          $.logError("Failed", plugin.url);
          failures.push({ plugin, output: result.output });
          break;
      }
    }
  }));
} finally {
  await tempDir.remove({ recursive: true });
}

for (const { plugin, output } of failures) {
  console.error(`\n=== ${plugin.name} (${plugin.url}) ===\n${output}`);
}
if (failures.length > 0) {
  console.error(`\n${failures.length} of ${plugins.length} plugins failed.`);
  Deno.exit(1);
}
console.log(`\nStarted ${plugins.length - skippedCount} plugins (${skippedCount} skipped).`);

async function testPlugin(plugin: PluginInfo, dir: ReturnType<typeof $.path>): Promise<PluginResult> {
  dir.mkdirSync({ recursive: true });
  const isProcessPlugin = new URL(plugin.url).pathname.endsWith(".json");
  dir.join("dprint.json").writeJsonPrettySync({
    ...(plugin.name in PLUGIN_CONFIGS ? { [plugin.configKey]: PLUGIN_CONFIGS[plugin.name] } : {}),
    // process plugins require a checksum, which `dprint add` resolves below
    plugins: isProcessPlugin ? [] : [plugin.url],
  });
  const run = (...args: string[]) =>
    $`${dprint} ${args}`
      .cwd(dir)
      // use a separate cache per plugin so that every plugin gets downloaded,
      // compiled, and set up from scratch without interfering with the others
      .env("DPRINT_CACHE_DIR", dir.join("cache").toString())
      .noThrow()
      .captureCombined();

  if (isProcessPlugin) {
    const result = await run("add", "--checksum", plugin.url);
    if (result.code !== 0) {
      // process plugins aren't necessarily published for every platform
      return /Unsupported (CPU architecture|operating system)/.test(result.combined)
        ? { kind: "skipped", reason: "not available for this platform" }
        : { kind: "failed", output: result.combined };
    }
  }
  // resolving the configuration downloads, compiles, and starts up the plugin
  const result = await run("output-resolved-config");
  return result.code === 0 ? { kind: "ok" } : { kind: "failed", output: result.combined };
}
