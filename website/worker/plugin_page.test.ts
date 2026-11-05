import { strictEqual } from "node:assert/strict";
import { getAddSpecifier, renderPluginDetailsHtml, renderPluginSummaryHtml } from "./plugin_page.ts";
import { readPluginInfo } from "./plugin_versions.ts";

const typescript = readPluginInfo({
  schemaVersion: 4,
  latest: [{
    name: "dprint-plugin-typescript",
    description: "TypeScript/JavaScript code formatter.",
    repoUrl: "https://github.com/dprint/dprint-plugin-typescript",
    npm: { name: "@dprint/typescript", version: "0.97.1" },
    configKey: "typescript",
    keywords: ["javascript", "typescript"],
    fileExtensions: ["ts", "tsx"],
    version: "0.97.1",
    url: "https://plugins.dprint.dev/typescript-0.97.1.wasm",
    downloadCount: { currentVersion: 666, allVersions: 979657 },
  }],
}).plugins.get("dprint-plugin-typescript")!;

Deno.test("gets the specifier of a plugin for dprint add", () => {
  strictEqual(getAddSpecifier("https://plugins.dprint.dev/typescript-0.97.1.wasm"), "typescript");
  strictEqual(getAddSpecifier("https://plugins.dprint.dev/jolars/panache-v1.4.1.wasm"), "jolars/panache");
  strictEqual(getAddSpecifier("https://plugins.dprint.dev/prettier-0.67.0.json"), "prettier");
  strictEqual(getAddSpecifier("https://example.com/plugin-0.1.0.wasm"), undefined);
});

Deno.test("renders a plugin's summary", () => {
  strictEqual(
    renderPluginSummaryHtml(typescript),
    "<p class=\"plugin-description\">TypeScript/JavaScript code formatter.</p>"
      + "<ul class=\"plugin-stats\"><li><span class=\"plugin-stat-value\">0.97.1</span> version</li>"
      + "<li><span class=\"plugin-stat-value\">979,657</span> downloads in the last 30 days</li></ul>",
  );
  // a page for another version
  strictEqual(renderPluginSummaryHtml(typescript, "0.96.0").includes(">0.96.0</span> version"), true);
});

Deno.test("renders a plugin's details", () => {
  const html = renderPluginDetailsHtml(typescript);

  strictEqual(html.startsWith("<h2>Install</h2><pre class=\"plugin-install\"><code>dprint add typescript</code></pre>"), true);
  strictEqual(html.includes("<dt>Downloads (30 days)</dt><dd>979,657</dd>"), true);
  strictEqual(html.includes("<dt>Files</dt><dd><ul class=\"plugin-chips\"><li><code>.ts</code></li><li><code>.tsx</code></li></ul></dd>"), true);
  strictEqual(
    html.includes(
      "<dt>Repository</dt><dd><a href=\"https://github.com/dprint/dprint-plugin-typescript\" rel=\"noopener noreferrer\">github.com/dprint/dprint-plugin-typescript</a></dd>",
    ),
    true,
  );
  strictEqual(html.includes("<a href=\"https://www.npmjs.com/package/@dprint/typescript\""), true);
});

Deno.test("leaves out what a plugin's info doesn't have or isn't safe to link to", () => {
  const html = renderPluginDetailsHtml({
    ...typescript,
    url: undefined,
    repoUrl: "javascript:alert(1)",
    npmName: undefined,
    description: "<b>bold</b>",
  });

  strictEqual(html.includes("Install"), false);
  strictEqual(html.includes("Repository"), false);
  strictEqual(html.includes("javascript:"), false);
  strictEqual(renderPluginSummaryHtml({ ...typescript, description: "<b>" }).includes("&lt;b&gt;"), true);
});
