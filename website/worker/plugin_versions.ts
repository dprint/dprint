// replaces the version placeholders in the docs' code blocks (ex.
// `"npm:@dprint/typescript@x.x.x"`) with the latest versions of the plugins

/** A version placeholder in a page's code. */
export type VersionPlaceholder =
  // ex. `"npm:@dprint/typescript@x.x.x"`
  | { kind: "npm"; name: string }
  // ex. `"https://plugins.dprint.dev/jolars/panache-x.x.x.wasm"` or, for a
  // process plugin, `"https://plugins.dprint.dev/drluckyspin/swift-vx.x.x.json@<checksum>"`
  | {
    kind: "url";
    /** The url with its version replaced, which matches it regardless of a leading `v`. */
    key: string;
    /** The plugin's `<username>/<name>` on plugins.dprint.dev, when it has one. */
    plugin: string | undefined;
    hasChecksum: boolean;
  };

/** The latest versions of the plugins in plugins.dprint.dev's info.json. */
export interface PluginInfo {
  /** The latest url of each plugin, by the url's key (see `getPluginUrlKey`). */
  urls: Map<string, string>;
  /** The latest version of each npm package. */
  npmVersions: Map<string, string>;
}

/** The latest release of a plugin, from its latest.json on plugins.dprint.dev. */
export interface LatestPluginRelease {
  url: string;
  checksum: string | undefined;
}

const npmPlaceholderPattern = /^"npm:(.+)@x\.x\.x"$/;
const urlPlaceholderPattern = /^"(https:\/\/plugins\.dprint\.dev\/(?:([A-Za-z0-9_-]+)\/)?[A-Za-z0-9_.-]+?-v?x\.x\.x\.(?:wasm|json))(@<checksum>)?"$/;
const pluginUrlVersionPattern = /-v?(?:x\.x\.x|[0-9]+\.[0-9]+\.[0-9]+(?:-[A-Za-z0-9.]+)?)\.(wasm|json)$/;

/** Gets the placeholder in the text of a string in a code block, if it is one. */
export function parseVersionPlaceholder(text: string): VersionPlaceholder | undefined {
  const npmMatch = npmPlaceholderPattern.exec(text);
  if (npmMatch != null) {
    return { kind: "npm", name: npmMatch[1]! };
  }
  const urlMatch = urlPlaceholderPattern.exec(text);
  if (urlMatch != null) {
    const user = urlMatch[2];
    const name = /\/([A-Za-z0-9_.-]+?)-v?x\.x\.x\./.exec(urlMatch[1]!)![1]!;
    return {
      kind: "url",
      key: getPluginUrlKey(urlMatch[1]!),
      plugin: user == null ? undefined : `${user}/${name}`,
      hasChecksum: urlMatch[3] != null,
    };
  }
  return undefined;
}

/**
 * Gets the url with its version replaced and without a leading `v`, so a
 * placeholder url matches the latest url (ex.
 * `https://plugins.dprint.dev/jolars/panache-x.x.x.wasm` matches
 * `https://plugins.dprint.dev/jolars/panache-v1.4.1.wasm`).
 */
export function getPluginUrlKey(url: string) {
  return url.replace(pluginUrlVersionPattern, "-x.x.x.$1");
}

/** Reads plugins.dprint.dev's info.json. */
export function readPluginInfo(json: unknown): PluginInfo {
  const info = json as { schemaVersion?: unknown; latest?: unknown };
  if (info.schemaVersion !== 4 || !Array.isArray(info.latest)) {
    throw new Error(`Unsupported plugin info schema version: ${info.schemaVersion}`);
  }
  const urls = new Map<string, string>();
  const npmVersions = new Map<string, string>();
  for (const plugin of info.latest as { url?: unknown; npm?: { name?: unknown; version?: unknown } }[]) {
    if (typeof plugin.url === "string") {
      urls.set(getPluginUrlKey(plugin.url), plugin.url);
    }
    if (typeof plugin.npm?.name === "string" && typeof plugin.npm.version === "string") {
      npmVersions.set(plugin.npm.name, plugin.npm.version);
    }
  }
  return { urls, npmVersions };
}

/** Reads a plugin's latest.json on plugins.dprint.dev. */
export function readLatestPluginRelease(json: unknown): LatestPluginRelease | undefined {
  const latest = json as { url?: unknown; checksum?: unknown } | undefined;
  if (typeof latest?.url !== "string") {
    return undefined;
  }
  return { url: latest.url, checksum: typeof latest.checksum === "string" ? latest.checksum : undefined };
}

/**
 * Gets the text a placeholder is replaced with, or undefined when the version
 * isn't known. A url is looked up in the info.json, except for one with a
 * checksum, which the info.json doesn't have, or one that isn't listed in it.
 */
export async function getPlaceholderReplacement(
  placeholder: VersionPlaceholder,
  info: PluginInfo | undefined,
  getLatestRelease: (plugin: string) => Promise<LatestPluginRelease | undefined>,
): Promise<string | undefined> {
  if (placeholder.kind === "npm") {
    const version = info?.npmVersions.get(placeholder.name);
    return version == null ? undefined : `"npm:${placeholder.name}@${version}"`;
  }
  const url = placeholder.hasChecksum ? undefined : info?.urls.get(placeholder.key);
  if (url != null) {
    return `"${url}"`;
  }
  if (placeholder.plugin == null) {
    return undefined;
  }
  const latest = await getLatestRelease(placeholder.plugin);
  if (latest == null || getPluginUrlKey(latest.url) !== placeholder.key) {
    return undefined;
  }
  if (placeholder.hasChecksum) {
    return latest.checksum == null ? undefined : `"${latest.url}@${latest.checksum}"`;
  }
  return `"${latest.url}"`;
}
