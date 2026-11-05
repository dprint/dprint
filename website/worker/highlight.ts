// syntax highlights the code examples, which the site's highlight.js theme styles

import type { LanguageFn } from "highlight.js";
import hljs from "highlight.js/lib/core";
import csharp from "highlight.js/lib/languages/csharp";
import css from "highlight.js/lib/languages/css";
import dockerfile from "highlight.js/lib/languages/dockerfile";
import go from "highlight.js/lib/languages/go";
import graphql from "highlight.js/lib/languages/graphql";
import ini from "highlight.js/lib/languages/ini";
import javascript from "highlight.js/lib/languages/javascript";
import json from "highlight.js/lib/languages/json";
import less from "highlight.js/lib/languages/less";
import markdown from "highlight.js/lib/languages/markdown";
import python from "highlight.js/lib/languages/python";
import rust from "highlight.js/lib/languages/rust";
import scss from "highlight.js/lib/languages/scss";
import sql from "highlight.js/lib/languages/sql";
import typescript from "highlight.js/lib/languages/typescript";
import vbnet from "highlight.js/lib/languages/vbnet";
import xml from "highlight.js/lib/languages/xml";
import yaml from "highlight.js/lib/languages/yaml";

const languages = { csharp, css, dockerfile, go, graphql, ini, javascript, json, less, markdown, python, rust, scss, sql, typescript, vbnet, xml, yaml };
for (const [name, language] of Object.entries(languages)) {
  // the type of a language module is inferred from its javascript, which is
  // stricter than what highlight.js accepts
  hljs.registerLanguage(name, language as LanguageFn);
}

// the languages of the file extensions the plugins on dprint.dev format
const languageByExtension: Record<string, keyof typeof languages> = {
  ts: "typescript",
  tsx: "typescript",
  mts: "typescript",
  cts: "typescript",
  js: "javascript",
  jsx: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  json: "json",
  jsonc: "json",
  json5: "json",
  md: "markdown",
  markdown: "markdown",
  toml: "ini",
  yaml: "yaml",
  yml: "yaml",
  css: "css",
  scss: "scss",
  less: "less",
  html: "xml",
  vue: "xml",
  svelte: "xml",
  astro: "xml",
  xml: "xml",
  sql: "sql",
  graphql: "graphql",
  gql: "graphql",
  dockerfile: "dockerfile",
  rs: "rust",
  py: "python",
  pyi: "python",
  cs: "csharp",
  vb: "vbnet",
  go: "go",
};

export interface HighlightedCode {
  /** The code as HTML with the highlight.js classes. */
  html: string;
  language: string;
}

/**
 * Highlights code in the language of a file extension, or returns undefined
 * when the extension isn't one of a known language.
 */
export function highlightCode(code: string, extension: string | undefined): HighlightedCode | undefined {
  const language = extension == null ? undefined : languageByExtension[extension.toLowerCase()];
  if (language == null) {
    return undefined;
  }
  return { html: hljs.highlight(code, { language }).value, language };
}
