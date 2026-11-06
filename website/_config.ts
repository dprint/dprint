import type Site from "lume/core/site.ts";
import lume from "lume/mod.ts";
import codeHighlight from "lume/plugins/code_highlight.ts";
import date from "lume/plugins/date.ts";
import esbuild from "lume/plugins/esbuild.ts";
import nunjucks from "lume/plugins/nunjucks.ts";
import redirects from "lume/plugins/redirects.ts";
import sass from "lume/plugins/sass.ts";
import anchor from "markdown-it-anchor";

await copyConfigSchema();

const site = lume({
  src: "./src",
  location: new URL("https://dprint.dev"),
}, {
  markdown: {
    options: {
      linkify: true,
    },
    plugins: [[anchor, {
      level: 2,
      permalink: anchor.permalink.headerLink(),
    }]],
  },
});

site
  .use(nunjucks())
  .use(sass())
  .use(date())
  .use(codeHighlight())
  // pages list the urls they moved from as `oldUrl`, which Cloudflare redirects to them
  .use(redirects({ output: writeCloudflareRedirects }))
  .use(esbuild({
    options: {
      bundle: true,
      format: "iife",
      target: "es2015",
      minify: false,
      entryPoints: ["scripts.js"],
    },
  }))
  .add("scripts.js")
  .add("style.scss")
  .copy("assets", ".")
  // response headers for Cloudflare to send with the static files
  .copy("_headers");

// gives the overview and configuration pages of each plugin in
// _data/pluginGroups.json the plugin's details, which layouts/plugin.njk shows
site.preprocess([".html"], (pages) => {
  const pageUrls = new Set(pages.map((page) => page.data.url));
  for (const page of pages) {
    if (page.data.layout !== "layouts/plugin.njk") {
      continue;
    }
    const slug = /^\/plugins\/([^/]+)\/(?:config\/)?$/.exec(page.data.url)?.[1];
    const plugin = (page.data.pluginGroups as { plugins: { slug: string }[] }[])
      .flatMap((group) => group.plugins)
      .find((plugin) => plugin.slug === slug);
    if (plugin == null) {
      throw new Error(`${page.data.url} uses the plugin layout, but isn't a plugin in _data/pluginGroups.json.`);
    }
    page.data.pluginPage = {
      ...plugin,
      tab: page.data.url.endsWith("/config/") ? "config" : "overview",
      hasConfig: pageUrls.has(`/plugins/${slug}/config/`),
    };
  }
});

// cache busting: give the built CSS/JS content-hashed filenames (like Vite)
// so each deploy invalidates stale browser caches, then rewrite the references
// in the generated HTML to point at the hashed names.
const hashedAssets = new Map<string, string>();

site.process([".css", ".js"], async (pages) => {
  for (const page of pages) {
    const url = page.data.url;
    const dot = url.lastIndexOf(".");
    const hashedUrl = `${url.slice(0, dot)}.${await shortHash(page.content!)}${url.slice(dot)}`;
    hashedAssets.set(url, hashedUrl);
    page.data.url = hashedUrl;
  }
});

site.process([".html"], (pages) => {
  for (const page of pages) {
    let html = page.content as string;
    for (const [from, to] of hashedAssets) {
      html = html.replaceAll(`"${from}"`, `"${to}"`);
    }
    page.content = html;
  }
});

export default site;

// writes the redirects in Cloudflare's `_redirects` file format
async function writeCloudflareRedirects(redirects: [string, string, number][], site: Site) {
  const lines = redirects.flatMap(([from, to, status]) => {
    // a redirect only matches the path as written, but pages are also served
    // without the trailing slash
    const froms = from.length > 1 && from.endsWith("/") ? [from, from.slice(0, -1)] : [from];
    return froms.map((from) => `${from} ${to} ${status}`);
  });
  const page = await site.getOrCreatePage("_redirects");
  page.text = lines.join("\n") + "\n";
}

async function shortHash(content: string | Uint8Array): Promise<string> {
  // copies the bytes because digest doesn't accept a view that might be over a SharedArrayBuffer
  const bytes = typeof content === "string" ? new TextEncoder().encode(content) : new Uint8Array(content);
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return Array.from(new Uint8Array(digest))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("")
    .slice(0, 10);
}

async function copyConfigSchema() {
  // the dprint CLI crate is the source of truth for the config schema (it
  // embeds the file at compile time for LSP completions). Pull it in here so
  // it's served at https://dprint.dev/schemas/v0.json. This generated file is
  // gitignored.
  const source = new URL("../crates/dprint/src/commands/lsp/config_schema.json", import.meta.url);
  const destDir = new URL("./src/assets/schemas/", import.meta.url);
  await Deno.mkdir(destDir, { recursive: true });
  await Deno.copyFile(source, new URL("v0.json", destDir));
}
