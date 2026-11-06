import { deepStrictEqual, strictEqual } from "node:assert/strict";
import { getRoutePath, getTagFromLatestInfo, getVersionPath, isValidPlugin, parseConfigRoute } from "./config_routes.ts";

Deno.test("parses configuration page routes", () => {
  deepStrictEqual(parseConfigRoute("/plugins/typescript/config/"), {
    kind: "latest",
    pagePath: "/plugins/typescript/config/",
    hasTrailingSlash: true,
  });
  deepStrictEqual(parseConfigRoute("/plugins/typescript/config"), {
    kind: "latest",
    pagePath: "/plugins/typescript/config/",
    hasTrailingSlash: false,
  });
  deepStrictEqual(parseConfigRoute("/plugins/pretty_yaml/0.97.0/config/"), {
    kind: "version",
    pagePath: "/plugins/pretty_yaml/config/",
    tag: "0.97.0",
    hasTrailingSlash: true,
  });
  deepStrictEqual(parseConfigRoute("/plugins/badness/v1.4.0/config"), {
    kind: "version",
    pagePath: "/plugins/badness/config/",
    tag: "v1.4.0",
    hasTrailingSlash: false,
  });
});

Deno.test("ignores other routes", () => {
  strictEqual(parseConfigRoute("/plugins/typescript/"), undefined);
  strictEqual(parseConfigRoute("/plugins/typescript/0.97.0/config/other/"), undefined);
  strictEqual(parseConfigRoute("/plugins/typescript/0.97.0-beta/config/"), undefined);
  strictEqual(parseConfigRoute("/plugins/typescript/a/b/config/"), undefined);
  strictEqual(parseConfigRoute("/config/"), undefined);
  strictEqual(parseConfigRoute("/plugins/typescript/config/index.html"), undefined);
});

Deno.test("gets the paths of routes", () => {
  strictEqual(getVersionPath("/plugins/typescript/config/", "0.97.1"), "/plugins/typescript/0.97.1/config/");
  strictEqual(getRoutePath(parseConfigRoute("/plugins/typescript/config")!), "/plugins/typescript/config/");
  strictEqual(getRoutePath(parseConfigRoute("/plugins/typescript/0.97.1/config")!), "/plugins/typescript/0.97.1/config/");
});

Deno.test("gets the tag from a plugin's latest.json", () => {
  strictEqual(getTagFromLatestInfo({ url: "https://plugins.dprint.dev/typescript-0.97.0.wasm" }), "0.97.0");
  // the version has the leading `v` stripped, but the tag doesn't
  strictEqual(
    getTagFromLatestInfo({ url: "https://plugins.dprint.dev/jolars/badness-v1.4.0.wasm", version: "1.4.0" }),
    "v1.4.0",
  );
  strictEqual(getTagFromLatestInfo({ url: "https://plugins.dprint.dev/dprint/prettier-0.67.0.json" }), "0.67.0");
  strictEqual(getTagFromLatestInfo({ version: "1.0.0" }), undefined);
  strictEqual(getTagFromLatestInfo(undefined), undefined);
});

Deno.test("validates plugins", () => {
  strictEqual(isValidPlugin("dprint/dprint-plugin-typescript"), true);
  strictEqual(isValidPlugin("g-plane/pretty_yaml"), true);
  strictEqual(isValidPlugin("dprint"), false);
  strictEqual(isValidPlugin("dprint/../other"), false);
});
