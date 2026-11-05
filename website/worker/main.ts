// The Cloudflare Worker for dprint.dev. It serves the static site built by
// Lume, except for the plugin configuration pages, which it renders from the
// plugin's schema.json on plugins.dprint.dev, and it replaces the plugin
// version placeholders in the pages it runs for with the latest versions.
import {
  type ConfigRoute,
  type ConfigTable,
  getLatestInfoUrl,
  getRoutePath,
  getSchemaUrl,
  getTagFromLatestInfo,
  getVersionPath,
  isValidPlugin,
  parseConfigRoute,
} from "./config_routes.ts";
import { ConfigTooLargeError, escapeHtml, getConfigSections, renderConfigErrorHtml, renderConfigHtml } from "./config_schema.ts";
import { type PageVersion, renderPluginDetailsHtml, renderPluginSummaryHtml } from "./plugin_page.ts";
import {
  getPlaceholderReplacement,
  type LatestPluginRelease,
  parseVersionPlaceholder,
  type PluginInfo,
  readLatestPluginRelease,
  readPluginInfo,
} from "./plugin_versions.ts";
import { readJsonWithSizeLimit, ResponseTooLargeError } from "./read_json.ts";

interface Env {
  ASSETS: Fetcher;
  // service binding to the plugins.dprint.dev worker, so requests to it don't
  // depend on how a worker on the same zone is routed. it's optional so the
  // worker can also fetch it over the internet
  PLUGINS?: Fetcher;
}

// bump this to stop using any configuration html that was cached by a previous deploy
const RENDER_VERSION = 3;
// how long the latest versions of plugins are used for
const LATEST_RELEASE_CACHE_MS = 5 * 60 * 1_000;
const MAX_MEMORY_CACHE_ENTRIES = 100;
// the most html kept in memory, which is several of the largest pages
const MAX_MEMORY_CACHE_HTML_LENGTH = 10_000_000;

// a release's schema doesn't change, so neither does the html rendered from it.
// this is only shorter than forever so forgetting to bump RENDER_VERSION
// doesn't last long
const RENDERED_CACHE_SECONDS = 7 * 24 * 60 * 60;
// for versions without a schema or with one that's too large, so requests for
// them don't each fetch and render it
const MISSING_CACHE_SECONDS = 5 * 60;
// stores the status of a cached response in the cache api
const STATUS_HEADER = "x-config-status";
// the most of a plugin's files that are read, which is about 10 times the size
// of the largest schema (dprint-plugin-typescript's with its code examples)
const MAX_SCHEMA_BYTES = 1_000_000;
const MAX_LATEST_INFO_BYTES = 100_000;
const MAX_PLUGIN_INFO_BYTES = 1_000_000;
const PLUGIN_INFO_URL = "https://plugins.dprint.dev/info.json";

const configHtmlMemoryCache = new Map<string, ConfigHtmlResult & { expiresAt: number }>();
const latestReleaseMemoryCache = new Map<string, { release: LatestPluginRelease; expiresAt: number }>();
let pluginInfoMemoryCache: { info: PluginInfo; expiresAt: number } | undefined;

export default {
  async fetch(request: Request, env: Env, ctx: ExecutionContext): Promise<Response> {
    const url = new URL(request.url);
    if (request.method !== "GET" && request.method !== "HEAD") {
      return env.ASSETS.fetch(request);
    }
    const route = parseConfigRoute(url.pathname);
    if (route == null) {
      return await servePage(request, url, env);
    }
    if (!route.hasTrailingSlash) {
      // the rest of the site uses trailing slashes
      return redirect(301, getRoutePath(route) + url.search);
    }
    if (route.kind === "version" && route.tag === "latest") {
      // the latest version is at the url without a version
      return redirect(301, route.pagePath + url.search);
    }
    return await handleConfigPage(request, url, route, env, ctx);
  },
};

/**
 * Renders a plugin's configuration page. The page without a version shows the
 * latest version and is the only one search engines index, so its url doesn't
 * change with each release.
 */
async function handleConfigPage(request: Request, url: URL, route: ConfigRoute, env: Env, ctx: ExecutionContext) {
  const page = await fetchConfigPage(url, route.pagePath, env);
  if (page instanceof Response) {
    // the page moved (ex. `/plugins/sh/config/` to `/plugins/shuck/config/`),
    // so go to the same version of where it moved to
    const location = page.headers.get("location")!;
    const movedRoute = parseConfigRoute(new URL(location, url).pathname);
    const movedPath = movedRoute == null
      ? location
      : getRoutePath(route.kind === "latest" ? movedRoute : { ...route, pagePath: movedRoute.pagePath });
    return redirect(page.status, movedPath + url.search);
  } else if (page == null) {
    // serves the static page, or the 404 page when there isn't one
    return env.ASSETS.fetch(request);
  }

  const results = new Map<ConfigTable, ConfigHtmlResult>();
  for (const table of page.tables) {
    results.set(table, await getTableConfigHtml(table, route, env, ctx));
  }
  const status = Math.max(...[...results.values()].map((r) => r.status));
  return rewriteConfigPage(page, (table) => results.get(table)!.html, {
    status,
    noIndex: route.kind === "version",
    info: await getPluginInfo(env),
    pageVersion: await getPageVersion(page.tables[0]!, route, env),
  });
}

async function getTableConfigHtml(table: ConfigTable, route: ConfigRoute, env: Env, ctx: ExecutionContext) {
  const tag = route.kind === "version" ? route.tag : await getLatestTag(table.plugin, env);
  if (tag == null) {
    return {
      html: renderConfigErrorHtml("Failed to get the latest version of this plugin.", {
        href: `https://plugins.dprint.dev/${table.plugin}/latest/schema.json`,
        text: "View the configuration schema.",
      }),
      status: 503,
      isCacheable: false,
    };
  }
  return await getConfigHtml(table, tag, route.pagePath, env, ctx);
}

// gets the version a configuration page is for, which the sidebar shows with
// a link to the page of the latest version or to the page of this version
async function getPageVersion(table: ConfigTable, route: ConfigRoute, env: Env): Promise<PageVersion | undefined> {
  if (route.kind === "version") {
    return { version: route.tag, link: { href: route.pagePath, text: "View the latest version" } };
  }
  const tag = await getLatestTag(table.plugin, env);
  return tag == null
    ? undefined
    : { version: tag, link: { href: getVersionPath(route.pagePath, tag), text: "Link to this version" } };
}

function redirect(status: number, location: string) {
  return new Response(null, { status, headers: { location } });
}

/**
 * Serves a static file. A page has its plugin version placeholders (ex.
 * `"npm:@dprint/typescript@x.x.x"`) replaced with the latest versions, and a
 * plugin's page has its info filled in.
 */
async function servePage(request: Request, url: URL, env: Env) {
  const isPage = request.method === "GET" && isPagePath(url.pathname);
  if (!isPage) {
    return env.ASSETS.fetch(request);
  }
  // a page's versions change without the static page changing, so the
  // browser's cached page isn't revalidated against the static page
  const headers = new Headers(request.headers);
  headers.delete("if-none-match");
  headers.delete("if-modified-since");
  const response = await env.ASSETS.fetch(new Request(request, { headers }));
  if (response.status !== 200 || !response.headers.get("content-type")?.startsWith("text/html")) {
    return response;
  }
  return rewritePage(response, await getPluginInfo(env), env);
}

// a page's path ends with a slash or doesn't have an extension (ex. `/plugins/typescript/`)
function isPagePath(pathname: string) {
  return pathname.endsWith("/") || !pathname.slice(pathname.lastIndexOf("/") + 1).includes(".");
}

function rewritePage(response: Response, info: PluginInfo | undefined, env: Env) {
  const headers = new Headers(response.headers);
  // the content no longer matches the static page these describe
  headers.delete("content-length");
  headers.delete("etag");
  headers.delete("last-modified");
  let text = "";
  return addPluginInfoHandlers(new HTMLRewriter(), info)
    // the strings in the highlighted code blocks
    .on("span.hljs-string", {
      async text(chunk) {
        // a string's text may be in several chunks, which are replaced by the last one
        text += chunk.text;
        if (!chunk.lastInTextNode) {
          chunk.remove();
          return;
        }
        const originalHtml = text;
        text = "";
        const placeholder = parseVersionPlaceholder(decodeHtmlText(originalHtml));
        const replacement = placeholder == null
          ? undefined
          : await getPlaceholderReplacement(placeholder, info, (plugin) => getLatestPluginRelease(plugin, env));
        chunk.replace(replacement == null ? originalHtml : escapeHtml(replacement), { html: true });
      },
    })
    .transform(new Response(response.body, { status: response.status, headers }));
}

/**
 * Fills in the summary and details of a plugin's page from its info, which
 * are left empty when it doesn't have any. A page for a version other than the
 * latest shows that version.
 */
function addPluginInfoHandlers(rewriter: HTMLRewriter, info: PluginInfo | undefined, pageVersion?: PageVersion) {
  return rewriter
    .on("[data-plugin-summary]", {
      element(element) {
        const plugin = getPluginDetails(element, "data-plugin-summary", info);
        if (plugin != null) {
          element.setInnerContent(renderPluginSummaryHtml(plugin), { html: true });
        }
      },
    })
    .on("[data-plugin-details]", {
      element(element) {
        const plugin = getPluginDetails(element, "data-plugin-details", info);
        if (plugin != null) {
          element.setInnerContent(renderPluginDetailsHtml(plugin, pageVersion), { html: true });
        }
      },
    });
}

function getPluginDetails(element: RewriterElement, attribute: string, info: PluginInfo | undefined) {
  const name = element.getAttribute(attribute);
  return name == null ? undefined : info?.plugins.get(name);
}

// decodes the characters escaped in the text of the static pages
function decodeHtmlText(html: string) {
  return html
    .replaceAll("&lt;", "<")
    .replaceAll("&gt;", ">")
    .replaceAll("&quot;", "\"")
    .replaceAll("&#39;", "'")
    .replaceAll("&amp;", "&");
}

interface ConfigPage {
  html: string;
  headers: Headers;
  tables: ConfigTable[];
}

/**
 * Gets the static page for the route when it's a configuration page or the
 * redirect response when the page moved.
 */
async function fetchConfigPage(url: URL, pagePath: string, env: Env): Promise<ConfigPage | Response | undefined> {
  // don't follow the redirects in _redirects, which would serve the page at its old url
  const response = await env.ASSETS.fetch(new URL(pagePath, url), { redirect: "manual" });
  if (response.status >= 300 && response.status < 400 && response.headers.has("location")) {
    await response.body?.cancel();
    return response;
  }
  if (response.status !== 200 || !response.headers.get("content-type")?.startsWith("text/html")) {
    await response.body?.cancel();
    return undefined;
  }
  const html = await response.text();
  const tables = await readConfigTables(html);
  return tables.length === 0 ? undefined : { html, headers: response.headers, tables };
}

async function readConfigTables(html: string) {
  const tables: ConfigTable[] = [];
  await new HTMLRewriter()
    .on(".plugin-config-table", {
      element(element) {
        const table = getConfigTable(element);
        if (table != null) {
          tables.push(table);
        }
      },
    })
    .transform(new Response(html))
    .arrayBuffer();
  return tables;
}

function rewriteConfigPage(
  page: ConfigPage,
  getHtml: (table: ConfigTable) => string,
  options: { status: number; noIndex?: boolean; info: PluginInfo | undefined; pageVersion: PageVersion | undefined },
) {
  const headers = new Headers(page.headers);
  // the content no longer matches the static page these describe
  headers.delete("content-length");
  headers.delete("etag");
  if (options.noIndex) {
    headers.set("x-robots-tag", "noindex");
  }
  let tableIndex = 0;
  return addPluginInfoHandlers(new HTMLRewriter(), options.info, options.pageVersion)
    .on(".plugin-config-table", {
      element(element) {
        if (getConfigTable(element) != null) {
          element.setInnerContent(getHtml(page.tables[tableIndex++]!), { html: true });
        }
      },
    })
    .transform(new Response(page.html, { status: options.status, headers }));
}

function getConfigTable(element: RewriterElement): ConfigTable | undefined {
  const plugin = element.getAttribute("data-plugin");
  if (plugin == null || !isValidPlugin(plugin)) {
    console.error(`Invalid data-plugin attribute on a .plugin-config-table: ${plugin}`);
    return undefined;
  }
  return { plugin, configKey: element.getAttribute("data-config-key") ?? undefined };
}

interface ConfigHtmlResult {
  html: string;
  status: number;
  /** Set when the html can be cached, which is when it's not a temporary failure. */
  isCacheable: boolean;
}

async function getConfigHtml(
  table: ConfigTable,
  tag: string,
  pagePath: string,
  env: Env,
  ctx: ExecutionContext,
): Promise<ConfigHtmlResult> {
  const cacheKey = `v${RENDER_VERSION}/${table.plugin}/${tag}/${table.configKey ?? ""}`;
  const memoryResult = configHtmlMemoryCache.get(cacheKey);
  if (memoryResult != null && memoryResult.expiresAt > Date.now()) {
    return memoryResult;
  }
  // the cache api is per data center, so this saves the other isolates there
  // from fetching and rendering the schema (it's a no-op on workers.dev)
  const cacheRequest = new Request(`https://dprint.dev/__worker-cache/config-html/${cacheKey}`);
  const cachedResponse = await getDefaultCache().match(cacheRequest);
  if (cachedResponse != null) {
    const result = {
      html: await cachedResponse.text(),
      status: Number(cachedResponse.headers.get(STATUS_HEADER) ?? 200),
      isCacheable: true,
    };
    setConfigHtmlMemoryCacheEntry(cacheKey, result);
    return result;
  }

  const result = await fetchAndRenderConfigHtml(table, tag, pagePath, env);
  if (result.isCacheable) {
    setConfigHtmlMemoryCacheEntry(cacheKey, result);
    ctx.waitUntil(
      getDefaultCache().put(
        cacheRequest,
        new Response(result.html, {
          headers: {
            "cache-control": `public, max-age=${getConfigHtmlCacheSeconds(result)}`,
            [STATUS_HEADER]: result.status.toString(),
          },
        }),
      ),
    );
  }
  return result;
}

async function fetchAndRenderConfigHtml(
  table: ConfigTable,
  tag: string,
  pagePath: string,
  env: Env,
): Promise<ConfigHtmlResult> {
  const schemaUrl = getSchemaUrl(table.plugin, tag);
  try {
    const response = await fetchFromPluginsSite(schemaUrl, env);
    if (response.status === 404) {
      await response.body?.cancel();
      return {
        html: renderConfigErrorHtml(`Version ${tag} of this plugin doesn't have a configuration schema.`, {
          href: pagePath,
          text: "View the configuration of the latest version.",
        }),
        status: 404,
        isCacheable: true,
      };
    }
    if (!response.ok) {
      await response.body?.cancel();
      throw new Error(`Failed fetching ${schemaUrl}: ${response.status}`);
    }
    const html = renderConfigHtml({
      schemaUrl,
      sections: getConfigSections(await readJsonWithSizeLimit(response, MAX_SCHEMA_BYTES), table.configKey),
    });
    return { html, status: 200, isCacheable: true };
  } catch (err) {
    console.error(err);
    if (err instanceof ResponseTooLargeError || err instanceof ConfigTooLargeError) {
      // cached like a missing schema, because a release's schema doesn't change
      return {
        html: renderConfigErrorHtml("This plugin's configuration schema is too large to display.", {
          href: schemaUrl,
          text: "View the configuration schema.",
        }),
        status: 502,
        isCacheable: true,
      };
    }
    // not cached, because it's probably temporary
    return {
      html: renderConfigErrorHtml("Failed to load the configuration.", {
        href: schemaUrl,
        text: "View the configuration schema.",
      }),
      status: 502,
      isCacheable: false,
    };
  }
}

function setConfigHtmlMemoryCacheEntry(cacheKey: string, result: ConfigHtmlResult) {
  setMemoryCacheEntry(configHtmlMemoryCache, cacheKey, {
    ...result,
    expiresAt: Date.now() + getConfigHtmlCacheSeconds(result) * 1_000,
  });
  // also limits the cache by the size of its html, removing the oldest entries
  let htmlLength = 0;
  for (const entry of configHtmlMemoryCache.values()) {
    htmlLength += entry.html.length;
  }
  for (const [key, entry] of configHtmlMemoryCache) {
    if (htmlLength <= MAX_MEMORY_CACHE_HTML_LENGTH || key === cacheKey) {
      break;
    }
    configHtmlMemoryCache.delete(key);
    htmlLength -= entry.html.length;
  }
}

function getConfigHtmlCacheSeconds(result: ConfigHtmlResult) {
  return result.status === 200 ? RENDERED_CACHE_SECONDS : MISSING_CACHE_SECONDS;
}

async function getLatestTag(plugin: string, env: Env) {
  const release = await getLatestPluginRelease(plugin, env);
  const tag = release == null ? undefined : getTagFromLatestInfo(release);
  if (release != null && tag == null) {
    console.error(`Failed getting the tag from the latest url of ${plugin}: ${release.url}`);
  }
  return tag;
}

/** Gets a plugin's latest release from its latest.json, or undefined when that fails. */
async function getLatestPluginRelease(plugin: string, env: Env) {
  if (!isValidPlugin(plugin)) {
    return undefined;
  }
  const cached = latestReleaseMemoryCache.get(plugin);
  if (cached != null && cached.expiresAt > Date.now()) {
    return cached.release;
  }
  try {
    const response = await fetchFromPluginsSite(getLatestInfoUrl(plugin), env);
    if (!response.ok) {
      await response.body?.cancel();
      throw new Error(`Failed fetching the latest info for ${plugin}: ${response.status}`);
    }
    const release = readLatestPluginRelease(await readJsonWithSizeLimit(response, MAX_LATEST_INFO_BYTES));
    if (release == null) {
      throw new Error(`Failed reading the latest info for ${plugin}.`);
    }
    setMemoryCacheEntry(latestReleaseMemoryCache, plugin, { release, expiresAt: Date.now() + LATEST_RELEASE_CACHE_MS });
    return release;
  } catch (err) {
    console.error(err);
    return undefined;
  }
}

/** Gets the latest versions of the plugins from info.json, or undefined when that fails. */
async function getPluginInfo(env: Env) {
  if (pluginInfoMemoryCache != null && pluginInfoMemoryCache.expiresAt > Date.now()) {
    return pluginInfoMemoryCache.info;
  }
  try {
    const response = await fetchFromPluginsSite(PLUGIN_INFO_URL, env);
    if (!response.ok) {
      await response.body?.cancel();
      throw new Error(`Failed fetching ${PLUGIN_INFO_URL}: ${response.status}`);
    }
    const info = readPluginInfo(await readJsonWithSizeLimit(response, MAX_PLUGIN_INFO_BYTES));
    pluginInfoMemoryCache = { info, expiresAt: Date.now() + LATEST_RELEASE_CACHE_MS };
    return info;
  } catch (err) {
    console.error(err);
    return undefined;
  }
}

// follows redirects manually so the ones back to plugins.dprint.dev go
// through the service binding as well
async function fetchFromPluginsSite(url: string, env: Env) {
  for (let i = 0; i < 5; i++) {
    const isPluginsSite = new URL(url).hostname === "plugins.dprint.dev";
    const response = isPluginsSite && env.PLUGINS != null
      ? await env.PLUGINS.fetch(url, { redirect: "manual" })
      : await fetch(url, { redirect: "manual" });
    const location = response.headers.get("location");
    if (response.status < 300 || response.status >= 400 || location == null) {
      return response;
    }
    await response.body?.cancel();
    url = new URL(location, url).href;
  }
  throw new Error(`Too many redirects fetching ${url}`);
}

function setMemoryCacheEntry<T>(cache: Map<string, T>, key: string, value: T) {
  cache.delete(key);
  cache.set(key, value);
  if (cache.size > MAX_MEMORY_CACHE_ENTRIES) {
    // maps iterate in insertion order, so this is the oldest entry
    cache.delete(cache.keys().next().value!);
  }
}

function getDefaultCache() {
  return (caches as unknown as { default: Cache }).default;
}

// the parts of the Cloudflare Workers runtime types used above

interface Fetcher {
  fetch(input: string | URL | Request, init?: RequestInit): Promise<Response>;
}

interface ExecutionContext {
  waitUntil(promise: Promise<unknown>): void;
}

interface RewriterElement {
  getAttribute(name: string): string | null;
  setInnerContent(content: string, options?: { html: boolean }): RewriterElement;
}

interface RewriterText {
  readonly text: string;
  readonly lastInTextNode: boolean;
  replace(content: string, options?: { html: boolean }): RewriterText;
  remove(): RewriterText;
}

declare class HTMLRewriter {
  on(
    selector: string,
    handlers: {
      element?(element: RewriterElement): void | Promise<void>;
      text?(text: RewriterText): void | Promise<void>;
    },
  ): HTMLRewriter;
  transform(response: Response): Response;
}
