import lume from "lume/mod.ts";
import codeHighlight from "lume/plugins/code_highlight.ts";
import date from "lume/plugins/date.ts";
import esbuild from "lume/plugins/esbuild.ts";
import nunjucks from "lume/plugins/nunjucks.ts";
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
  .copy("assets", ".");

export default site;

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
