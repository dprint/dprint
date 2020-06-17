// @ts-check
const showdown = require("showdown");
const hljs = require("highlight.js");
const fs = require("fs");
const { minify: htmlMinify } = require("html-minifier");
const CleanCss = require("clean-css");
const sass = require("node-sass");
const jsMinify = require("node-minify");

const titleInjectText = "<!-- title-inject -->";
const descriptionInjectText = "<!-- description-inject -->";
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
    "dprint - Code Formatter",
    "A pluggable and configurable code formatting platform written in Rust.",
    indexHtmlText,
    true,
);

buildForPath("pricing", fullPageHtmlPageText);
buildForPath("thank-you", fullPageHtmlPageText);
buildForPath("privacy-policy", fullPageHtmlPageText);
buildForPath("contact", fullPageHtmlPageText);

buildForPath("cli", documentationHtmlPageText);
buildForPath("config", documentationHtmlPageText);
buildForPath("install", documentationHtmlPageText);
buildForPath("plugin-dev", documentationHtmlPageText);
buildForPath("plugins", documentationHtmlPageText);
buildForPath("setup", documentationHtmlPageText);
buildForPath("plugins/typescript", documentationHtmlPageText);
buildForPath("plugins/typescript/config", documentationHtmlPageText);
buildForPath("plugins/json", documentationHtmlPageText);
buildForPath("plugins/json/config", documentationHtmlPageText);
buildForPath("plugins/rustfmt", documentationHtmlPageText);

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
    let html;
    let metaData;
    if (subTemplateText != null) {
        const mdResult = processMarkdown(mdText);
        html = subTemplateText.replace(injectText, mdResult.html);
        metaData = mdResult.metaData;
    } else {
        const mdResult = processMarkdown(mdText);
        html = mdResult.html;
        metaData = mdResult.metaData;
    }
    writeHtmlFile(
        "build-website/" + filePath + "/index.html",
        getTitle(),
        getDescription(),
        html,
        // @ts-ignore
        metaData.robots !== "false",
    );

    function getTitle() {
        if (metaData.title == null) {
            throw new Error("Could not find title metadata for " + filePath);
        }
        return metaData.title + " - dprint - Code Formatter";
    }

    function getDescription() {
        if (metaData.description == null) {
            throw new Error("Could not find description metadata for " + filePath);
        }
        return metaData.description;
    }
}

/** @param {string} [filePath] - File path to write to. */
/** @param {string} [title] - Title of the html file. */
/** @param {string} [description] - Description of the html file. */
/** @param {string} [html] - Html to write. */
/** @param {boolean} [robots] - Set to false to disallow robots. */
function writeHtmlFile(filePath, title, description, html, robots) {
    html = templateHtmlPageText
        .replace(titleInjectText, title)
        .replace(descriptionInjectText, description)
        .replace(injectText, html)
        .replace(additionalInjectText, robots ? "" : "<meta name=\"robots\" content=\"noindex\">");
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

            throw new Error("Unknown language: " + left);
        }

        function htmlunencode(text) {
            return (text.replace(/&amp;/g, "&")
                .replace(/&lt;/g, "<")
                .replace(/&gt;/g, ">"));
        }
    });
}
