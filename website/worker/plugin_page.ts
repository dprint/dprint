// renders the parts of a plugin's page that come from plugins.dprint.dev's
// info.json, which the static page has empty elements for

import { escapeHtml } from "./config_schema.ts";
import { getPluginUrlKey, type PluginDetails } from "./plugin_versions.ts";

const numberFormat = new Intl.NumberFormat("en-US");

/**
 * Renders the summary below a plugin's name, which is its description, version,
 * and downloads. A page for a version other than the latest shows that version.
 */
export function renderPluginSummaryHtml(plugin: PluginDetails, version = plugin.version) {
  let html = "";
  if (plugin.description != null) {
    html += `<p class="plugin-description">${escapeHtml(plugin.description)}</p>`;
  }
  const stats: string[] = [];
  if (version != null) {
    stats.push(`<li><span class="plugin-stat-value">${escapeHtml(version)}</span> version</li>`);
  }
  if (plugin.monthlyDownloads != null) {
    stats.push(
      `<li><span class="plugin-stat-value">${numberFormat.format(plugin.monthlyDownloads)}</span> downloads in the last 30 days</li>`,
    );
  }
  if (stats.length > 0) {
    html += `<ul class="plugin-stats">${stats.join("")}</ul>`;
  }
  return html;
}

/** Renders the details beside a plugin's page, like the command to install it and its links. */
export function renderPluginDetailsHtml(plugin: PluginDetails) {
  let html = "";
  const specifier = plugin.url == null ? undefined : getAddSpecifier(plugin.url);
  if (specifier != null) {
    html += `<h2>Install</h2><pre class="plugin-install"><code>dprint add ${escapeHtml(specifier)}</code></pre>`;
  }
  const details: [string, string][] = [];
  if (plugin.version != null) {
    details.push(["Version", escapeHtml(plugin.version)]);
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
    details.push(["Repository", renderLink(repoUrl.href, repoUrl.host + repoUrl.pathname.replace(/\/$/, ""))]);
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
