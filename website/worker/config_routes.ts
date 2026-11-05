// urls of the plugin configuration pages

/** A request for a plugin's configuration page. */
export type ConfigRoute =
  // `/plugins/<name>/config/`, which redirects to the latest version
  | { kind: "latest"; pagePath: string }
  // `/plugins/<name>/config/<tag>/`
  | { kind: "version"; pagePath: string; tag: string; hasTrailingSlash: boolean };

/** A plugin configuration table declared in a page. */
export interface ConfigTable {
  /** The plugin's `<username>/<repo>` on plugins.dprint.dev. */
  plugin: string;
  /** Set when the schema nests the plugin config under this key. */
  configKey: string | undefined;
}

// same as the tags plugins.dprint.dev accepts, which can't contain a dash
const tagPattern = "[A-Za-z0-9._]+";
const configPagePattern = new RegExp(`^(/plugins/[A-Za-z0-9_.-]+/config)(?:/(${tagPattern}))?(/?)$`);
const pluginPattern = /^[A-Za-z0-9-]+\/[A-Za-z0-9_.-]+$/;
// the tag at the end of the url in a latest.json (ex. `typescript-0.97.0.wasm`)
const latestUrlTagPattern = new RegExp(`-(${tagPattern})\\.(?:wasm|json)$`);

export function parseConfigRoute(pathname: string): ConfigRoute | undefined {
  const match = configPagePattern.exec(pathname);
  if (match == null) {
    return undefined;
  }
  const pagePath = match[1]! + "/";
  const tag = match[2];
  if (tag?.endsWith(".html")) {
    // a file (ex. `/plugins/typescript/config/index.html`), which isn't a version
    return undefined;
  }
  if (tag == null || tag === "latest") {
    return { kind: "latest", pagePath };
  }
  return { kind: "version", pagePath, tag, hasTrailingSlash: match[3] === "/" };
}

export function getVersionPath(pagePath: string, tag: string) {
  return `${pagePath}${tag}/`;
}

/** Gets the schema url of a plugin release. */
export function getSchemaUrl(plugin: string, tag: string) {
  return `https://plugins.dprint.dev/${plugin}/${tag}/schema.json`;
}

export function getLatestInfoUrl(plugin: string) {
  return `https://plugins.dprint.dev/${plugin}/latest.json`;
}

/**
 * Gets the release tag from the response of a plugin's latest.json.
 *
 * This uses the tag in the plugin url and not the `version` property because
 * the version has any leading `v` stripped from the tag (ex. `v1.4.0`).
 */
export function getTagFromLatestInfo(latestInfo: unknown) {
  const url = (latestInfo as { url?: unknown } | undefined)?.url;
  if (typeof url !== "string") {
    return undefined;
  }
  return latestUrlTagPattern.exec(url)?.[1];
}

export function isValidPlugin(plugin: string) {
  return pluginPattern.test(plugin);
}
