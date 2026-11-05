// renders the parts of a plugin's page that come from plugins.dprint.dev's
// info.json, which the static page has empty elements for

import { escapeHtml } from "./config_schema.ts";
import { getPluginUrlKey, type PluginDetails } from "./plugin_versions.ts";

const numberFormat = new Intl.NumberFormat("en-US");

/** The version of a plugin a page is for, which may not be the latest. */
export interface PageVersion {
  version: string;
  /** A link to the page of another version (ex. the latest). */
  link?: { href: string; text: string };
}

/** Renders the summary below a plugin's name, which is its description. */
export function renderPluginSummaryHtml(plugin: PluginDetails) {
  return plugin.description == null ? "" : `<p class="plugin-description">${escapeHtml(plugin.description)}</p>`;
}

/**
 * Renders the details beside a plugin's page, like the command to install it
 * and its links. The version is the latest unless the page is for another one.
 */
export function renderPluginDetailsHtml(plugin: PluginDetails, pageVersion?: PageVersion) {
  let html = "";
  const specifier = plugin.url == null ? undefined : getAddSpecifier(plugin.url);
  if (specifier != null) {
    const command = escapeHtml(`dprint add ${specifier}`);
    html += `<h2>Install</h2><div class="cmd-line plugin-install"><code>${command}</code>`
      + `<button type="button" class="copy-btn" data-copy="${command}">copy</button></div>`;
  }
  const details: [string, string][] = [];
  const version = pageVersion?.version ?? plugin.version;
  if (version != null) {
    const link = pageVersion?.link == null
      ? ""
      : `<a class="plugin-version-link" href="${escapeHtml(pageVersion.link.href)}">${escapeHtml(pageVersion.link.text)}</a>`;
    details.push(["Version", escapeHtml(version) + link]);
  }
  if (plugin.monthlyDownloads != null) {
    details.push(["Downloads (30 days)", numberFormat.format(plugin.monthlyDownloads)]);
  }
  if (plugin.configKey != null) {
    details.push(["Config key", `<code>${escapeHtml(plugin.configKey)}</code>`]);
  }
  const files = [...plugin.fileExtensions.map((extension) => `.${extension}`), ...plugin.fileNames];
  if (files.length > 0) {
    details.push(["Files", renderChips(files)]);
  }
  const repoUrl = plugin.repoUrl == null ? undefined : getHttpsUrl(plugin.repoUrl);
  if (repoUrl != null) {
    // a github repository is shown as its `<owner>/<name>`
    const path = repoUrl.pathname.replace(/^\/|\/$/g, "");
    details.push(["Repository", renderLink(repoUrl.href, repoUrl.host === "github.com" ? path : `${repoUrl.host}/${path}`)]);
  }
  if (plugin.npmName != null) {
    details.push(["npm", renderLink(`https://www.npmjs.com/package/${plugin.npmName}`, plugin.npmName)]);
  }
  if (plugin.keywords.length > 0) {
    details.push(["Keywords", renderChips(plugin.keywords)]);
  }
  if (details.length > 0) {
    html += `<dl class="plugin-details">${details.map(([term, value]) => `<dt>${term}</dt><dd>${value}</dd>`).join("")}</dl>`;
  }
  return html;
}

/**
 * Gets what `dprint add` takes for a plugin, which is its url on
 * plugins.dprint.dev without the version (ex. `typescript` or `jolars/panache`).
 */
export function getAddSpecifier(url: string) {
  return /^https:\/\/plugins\.dprint\.dev\/((?:[A-Za-z0-9_-]+\/)?[A-Za-z0-9_.-]+)-x\.x\.x\.(?:wasm|json)$/
    .exec(getPluginUrlKey(url))?.[1];
}

function renderChips(items: string[]) {
  return `<ul class="plugin-chips">${items.map((item) => `<li><code>${escapeHtml(item)}</code></li>`).join("")}</ul>`;
}

function renderLink(href: string, text: string) {
  return `<a href="${escapeHtml(href)}" rel="noopener noreferrer">${escapeHtml(text)}</a>`;
}

// only links to https urls, since they come from a plugin's info
function getHttpsUrl(text: string) {
  try {
    const url = new URL(text);
    return url.protocol === "https:" ? url : undefined;
  } catch {
    return undefined;
  }
}
