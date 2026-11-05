// urls of the plugin configuration pages

/**
 * A request for a plugin's configuration page, where `pagePath` is the path of
 * the static page the configuration is rendered into.
 */
export type ConfigRoute =
  // `/plugins/<name>/config/`, which shows the latest version and is the one
  // search engines index
  | { kind: "latest"; pagePath: string; hasTrailingSlash: boolean }
  // `/plugins/<name>/<tag>/config/`
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
const configPagePattern = new RegExp(`^/plugins/([A-Za-z0-9_.-]+)/(?:(${tagPattern})/)?config(/?)$`);
const pluginPattern = /^[A-Za-z0-9-]+\/[A-Za-z0-9_.-]+$/;
// the tag at the end of the url in a latest.json (ex. `typescript-0.97.0.wasm`)
const latestUrlTagPattern = new RegExp(`-(${tagPattern})\\.(?:wasm|json)$`);

export function parseConfigRoute(pathname: string): ConfigRoute | undefined {
  const match = configPagePattern.exec(pathname);
  if (match == null) {
    return undefined;
  }
  const pagePath = `/plugins/${match[1]}/config/`;
  const tag = match[2];
  const hasTrailingSlash = match[3] === "/";
  return tag == null
    ? { kind: "latest", pagePath, hasTrailingSlash }
    : { kind: "version", pagePath, tag, hasTrailingSlash };
}

/** Gets the path of a route, with a trailing slash like the rest of the site. */
export function getRoutePath(route: ConfigRoute) {
  return route.kind === "latest" ? route.pagePath : getVersionPath(route.pagePath, route.tag);
}

/** Gets the path of a version's configuration page (ex. `/plugins/typescript/0.97.1/config/`). */
export function getVersionPath(pagePath: string, tag: string) {
  return pagePath.replace(/config\/$/, `${tag}/config/`);
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
