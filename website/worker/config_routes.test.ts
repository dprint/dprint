import { deepStrictEqual, strictEqual } from "node:assert/strict";
import { getTagFromLatestInfo, isValidPlugin, parseConfigRoute } from "./config_routes.ts";

Deno.test("parses configuration page routes", () => {
  deepStrictEqual(parseConfigRoute("/plugins/typescript/config/"), {
    kind: "latest",
    pagePath: "/plugins/typescript/config/",
  });
  deepStrictEqual(parseConfigRoute("/plugins/typescript/config"), {
    kind: "latest",
    pagePath: "/plugins/typescript/config/",
  });
  deepStrictEqual(parseConfigRoute("/plugins/typescript/config/latest/"), {
    kind: "latest",
    pagePath: "/plugins/typescript/config/",
  });
  deepStrictEqual(parseConfigRoute("/plugins/pretty_yaml/config/0.97.0/"), {
    kind: "version",
    pagePath: "/plugins/pretty_yaml/config/",
    tag: "0.97.0",
    hasTrailingSlash: true,
  });
  deepStrictEqual(parseConfigRoute("/plugins/badness/config/v1.4.0"), {
    kind: "version",
    pagePath: "/plugins/badness/config/",
    tag: "v1.4.0",
    hasTrailingSlash: false,
  });
});

Deno.test("ignores other routes", () => {
  strictEqual(parseConfigRoute("/plugins/typescript/"), undefined);
  strictEqual(parseConfigRoute("/plugins/typescript/config/0.97.0/other/"), undefined);
  strictEqual(parseConfigRoute("/plugins/typescript/config/0.97.0-beta/"), undefined);
  strictEqual(parseConfigRoute("/config/"), undefined);
  strictEqual(parseConfigRoute("/plugins/typescript/config/index.html"), undefined);
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
