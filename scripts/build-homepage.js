// @ts-check

// Note: this started out super simple then become too complicated.
// This should be refactored to use an actual build tool.

const showdown = require("showdown");
const hljs = require("highlight.js");
const fs = require("fs");
const { minify: htmlMinify } = require("html-minifier");
const CleanCss = require("clean-css");
const sass = require("node-sass");
const jsMinify = require("node-minify");

buildWebsite();

function buildWebsite() {
  const additionalInjectText = "<!-- additional-inject -->";
  const injectText = "<!-- inject -->";

  initCodeHighlightExtension();

  const templateHtmlPageFilePath = "../website/templates/template.html";
  const templateHtmlPageText = fs.readFileSync(templateHtmlPageFilePath, { encoding: "utf8" });

  const documentationHtmlPageFilePath = "../website/templates/documentation.html";
  const documentationHtmlPageText = fs.readFileSync(documentationHtmlPageFilePath, { encoding: "utf8" });

  const fullPageHtmlPageFilePath = "../website/templates/full-page.html";
  const fullPageHtmlPageText = fs.readFileSync(fullPageHtmlPageFilePath, { encoding: "utf8" });

  const converter = new showdown.Converter({ extensions: ["codehighlight"], metadata: true });
  converter.setFlavor("github");

  // index.html
  const indexHtmlText = fs.readFileSync("../website/index.html", { encoding: "utf8" });
  writeHtmlFile(
    "build-website/index.html",
    {
      page_title: "dprint - Code Formatter",
      title: "dprint - Code Formatter",
      description: "A pluggable and configurable code formatting platform written in Rust.",
    },
    indexHtmlText,
  );

  buildForPath("sponsor", fullPageHtmlPageText);
  buildForPath("thank-you", fullPageHtmlPageText);
  buildForPath("contact", fullPageHtmlPageText);
  buildForPath("blog", fullPageHtmlPageText);

  buildForPath("cli", documentationHtmlPageText);
  buildForPath("ci", documentationHtmlPageText);
  buildForPath("config", documentationHtmlPageText);
  buildForPath("install", documentationHtmlPageText);
  buildForPath("overview", documentationHtmlPageText);
  buildForPath("plugin-dev", documentationHtmlPageText);
  buildForPath("plugins", documentationHtmlPageText);
  buildForPath("setup", documentationHtmlPageText);
  buildForPath("plugins/typescript", documentationHtmlPageText);
  buildForPath("plugins/typescript/config", documentationHtmlPageText);
  buildForPath("plugins/json", documentationHtmlPageText);
  buildForPath("plugins/json/config", documentationHtmlPageText);
  buildForPath("plugins/markdown", documentationHtmlPageText);
  buildForPath("plugins/markdown/config", documentationHtmlPageText);
  buildForPath("plugins/toml", documentationHtmlPageText);
  buildForPath("plugins/toml/config", documentationHtmlPageText);
  buildForPath("plugins/dockerfile", documentationHtmlPageText);
  buildForPath("plugins/dockerfile/config", documentationHtmlPageText);
  buildForPath("plugins/prettier", documentationHtmlPageText);
  buildForPath("plugins/roslyn", documentationHtmlPageText);
  buildForPath("plugins/rustfmt", documentationHtmlPageText);
  buildForPath("plugins/yapf", documentationHtmlPageText);

  createRedirect("pricing", "sponsor");

  // minify index.css
  const sassFilePath = "../website/css/style.scss";
  const indexCssPageText = sass.renderSync({ file: sassFilePath }).css;
  fs.writeFileSync("build-website/style.css", new CleanCss().minify(indexCssPageText).styles);

  // minify js files
  jsMinify.minify({
    compressor: "gcc",
    input: "../website/scripts/*.js",
    output: "build-website/scripts.js",
  });

  /** @param {string} [filePath] - Relative path to the file without extension. */
  /** @param {string} [subTemplateText] - Name of the sub template to use. */
  function buildForPath(filePath, subTemplateText) {
    const mdText = fs.readFileSync("../website/" + filePath + ".md", { encoding: "utf8" });
    fs.mkdirSync("build-website/" + filePath);
    const mdResult = processMarkdown(mdText);
    const html = subTemplateText.replace(injectText, mdResult.html);
    /** @type {any} */
    const metaData = mdResult.metaData;
    verifyMetaData();
    writeHtmlFile(
      "build-website/" + filePath + "/index.html",
      metaData,
      html,
    );

    function verifyMetaData() {
      if (metaData.title == null) {
        throw new Error("Could not find title metadata for " + filePath);
      }
      metaData.page_title = metaData.title + " - dprint - Code Formatter";
      if (metaData.description == null) {
        throw new Error("Could not find description metadata for " + filePath);
      }
    }
  }

  /** @param {string} [filePath] - File path to write to. */
  /** @param {any} [metaData] - Title of the html file. */
  /** @param {string} [html] - Html to write. */
  function writeHtmlFile(filePath, metaData, html) {
    html = templateHtmlPageText.replace(injectText, html);
    for (const prop of Object.keys(metaData)) {
      if (prop === "robots") {
        if (metaData[prop] === false) {
          html = html.replace(additionalInjectText, "<meta name=\"robots\" content=\"noindex\">");
        }
      } else if (prop === "author") {
        if (metaData[prop] !== "David Sherret") {
          throw new Error("Unhandled author.");
        }
      } else {
        const newText = html.replace(new RegExp("\{\{" + prop + "\}\}", "gi"), metaData[prop]);
        if (newText === html && prop !== "title") {
          throw new Error("The text did not change after applying meta data: " + prop);
        } else {
          html = newText;
        }
      }
    }

    html = html.replace(additionalInjectText, "");

    if (html.includes("{{") || html.includes("inject")) {
      console.log(html);
      throw new Error(`The page ${filePath} still had a template in it.`);
    }

    fs.writeFileSync(filePath, htmlMinify(html, { collapseWhitespace: true }));
  }

  /** @param {string} [mdText] - Markdown to process and inject. */
  function processMarkdown(mdText) {
    const html = converter.makeHtml(mdText);
    const metaData = converter.getMetadata() || {};
    return { html, metaData };
  }

  function initCodeHighlightExtension() {
    // from https://github.com/showdownjs/showdown/issues/215#issuecomment-168679324
    showdown.extension("codehighlight", function() {
      return [{
        type: "output",
        filter: function(text, converter, options) {
          // use new shodown's regexp engine to conditionally parse codeblocks
          const left = "<pre><code\\b[^>]*>";
          const right = "</code></pre>";
          const flags = "g";
          const replacement = (wholeMatch, match, left, right) => {
            // unescape match to prevent double escaping
            match = htmlunencode(match);
            return left + hljs.highlight(getLanguage(left), match).value + right;
          };
          return showdown.helper.replaceRecursiveRegExp(text, replacement, left, right, flags);
        },
      }];

      function getLanguage(left) {
        if (left.indexOf("-json") !== -1) {
          return "json";
        }
        if (left.indexOf("-js") !== -1 || left.indexOf("-javascript") !== -1) {
          return "javascript";
        }
        if (left.indexOf("-ts") !== -1 || left.indexOf("-typescript") !== -1) {
          return "typescript";
        }
        if (left.indexOf("-bash") !== -1) {
          return "bash";
        }
        if (left.indexOf("-powershell") !== -1) {
          return "powershell";
        }
        if (left.indexOf("-rust") !== -1) {
          return "rust";
        }
        if (left.indexOf("-text") !== -1) {
          return "text";
        }
        if (left.indexOf("-toml") !== -1) {
          return "toml";
        }
        if (left.indexOf("-md") !== -1 || left.indexOf("-markdown") !== -1) {
          return "markdown";
        }

        throw new Error("Unknown language: " + left);
      }

      function htmlunencode(text) {
        return (text.replace(/&amp;/g, "&")
          .replace(/&lt;/g, "<")
          .replace(/&gt;/g, ">"));
      }
    });
  }

  /** @param {string} [from] - Page to redirect from. */
  /** @param {string} [to] - Page to redirect to. */
  function createRedirect(from, to) {
    const text = `<html>
  <head>
    <meta http-equiv="refresh" content="0; url=https://dprint.dev/${to}">
    <meta name="robots" content="noindex">
  </head>
</html>`;
    fs.mkdirSync("build-website/" + from);
    fs.writeFileSync("build-website/" + from + "/index.html", text);
  }
}
