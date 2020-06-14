// @ts-check
const showdown = require("showdown");
const hljs = require("highlight.js");
const fs = require("fs");
const { minify: htmlMinify } = require("html-minifier");
const CleanCss = require("clean-css");
const sass = require("node-sass");
const jsMinify = require("node-minify");

const injectText = "<!-- inject -->";

initCodeHighlightExtension();

const templateHtmlPageFilePath = "../website/templates/template.html";
const templateHtmlPageText = fs.readFileSync(templateHtmlPageFilePath, { encoding: "utf8" });

const documentationHtmlPageFilePath = "../website/templates/documentation.html";
const documentationHtmlPageText = fs.readFileSync(documentationHtmlPageFilePath, { encoding: "utf8" });

const fullPageHtmlPageFilePath = "../website/templates/full-page.html";
const fullPageHtmlPageText = fs.readFileSync(fullPageHtmlPageFilePath, { encoding: "utf8" });

const converter = new showdown.Converter({ extensions: ["codehighlight"] });
converter.setFlavor("github");

// index.html
const indexHtmlText = fs.readFileSync("../website/index.html", { encoding: "utf8" });
fs.writeFileSync("build-website/index.html", templateHtmlPageText.replace(injectText, indexHtmlText));

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
// plugins/typescript/index.html
buildForPath("plugins/typescript", documentationHtmlPageText);
buildForPath("plugins/typescript/config", documentationHtmlPageText);
// plugins/json/index.html
buildForPath("plugins/json", documentationHtmlPageText);
buildForPath("plugins/json/config", documentationHtmlPageText);

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
    let htmlText;
    if (subTemplateText != null) {
        htmlText = processMarkdown(subTemplateText, mdText);
        htmlText = templateHtmlPageText.replace(injectText, htmlText);
    }
    else {
        htmlText = processMarkdown(templateHtmlPageText, mdText);
    }
    fs.writeFileSync("build-website/" + filePath + "/index.html", htmlText);
}

/** @param {string} [htmlText] - Html text to use. */
/** @param {string} [mdText] - Markdown to process and inject. */
function processMarkdown(htmlText, mdText) {
    const innerHtml = converter.makeHtml(mdText);
    return htmlMinify(htmlText.replace(injectText, innerHtml), { collapseWhitespace: true });
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
            if (left.indexOf("-json") !== -1)
                return "json";
            if (left.indexOf("-js") !== -1 || left.indexOf("-javascript") !== -1)
                return "javascript";
            if (left.indexOf("-ts") !== -1 || left.indexOf("-typescript") !== -1)
                return "typescript";
            if (left.indexOf("-bash") !== -1)
                return "bash";
            if (left.indexOf("-powershell") !== -1)
                return "powershell";
            if (left.indexOf("-rust") !== -1)
                return "rust";

            throw new Error("Unknown language: " + left);
        }

        function htmlunencode(text) {
            return (text.replace(/&amp;/g, "&")
                .replace(/&lt;/g, "<")
                .replace(/&gt;/g, ">"));
        }
    });
}
