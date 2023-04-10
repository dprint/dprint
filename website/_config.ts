import { tgz } from "https://deno.land/x/compress@v0.4.1/mod.ts";
import lume from "lume/mod.ts";
import codeHighlight from "lume/plugins/code_highlight.ts";
import date from "lume/plugins/date.ts";
import esbuild from "lume/plugins/esbuild.ts";
import sass from "lume/plugins/sass.ts";
import anchor from "npm:markdown-it-anchor@8.6.7";

await buildSass();

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
  .use(sass())
  .use(codeHighlight())
  .use(date())
  .use(esbuild({
    options: {
      bundle: true,
      format: "iife",
      target: "es2015",
      minify: false,
      entryPoints: ["scripts.js"],
    },
  }))
  // need to ignore this for some reason
  .ignore("scripts")
  .copy("assets", ".");

export default site;

async function buildSass() {
  // sass doesn't support remote urls and I'm too lazy to switch away
  // to anything else at the moment, so we download the bulma-scss
  // package and extract it to a folder before building

  if (await directoryExists("./src/_includes/css/bulma")) {
    return;
  }

  const response = await fetch("https://registry.npmjs.org/bulma-scss/-/bulma-scss-0.9.3.tgz");
  if (!response.ok) {
    throw new Error(response.statusText);
  }
  const data = await response.arrayBuffer();
  await Deno.writeFile("./data.tgz", new Uint8Array(data));
  await tgz.uncompress("./data.tgz", "./src/_includes/css/bulma");
  await Deno.remove("./data.tgz");

  async function directoryExists(path: string) {
    try {
      await Deno.stat(path);
      return true;
    } catch {
      return false;
    }
  }
}
