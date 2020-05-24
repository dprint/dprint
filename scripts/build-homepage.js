// @ts-check
const showdown = require("showdown");
const hljs = require("highlight.js");
const fs = require("fs");
const { minify } = require("html-minifier");
const CleanCss = require("clean-css");

initCodeHighlightExtension();

const templateHtmlPageFilePath = "build-website/template.html";
const templateHtmlPageText = fs.readFileSync(templateHtmlPageFilePath, { encoding: "utf8" });

const converter = new showdown.Converter({ extensions: ["codehighlight"] });
converter.setFlavor("github");

// index.html
const indexMd = fs.readFileSync("docs/home.md", { encoding: "utf8" });
fs.writeFileSync("build-website/index.html", processMarkdown(indexMd));

// sponsor/index.html
buildForPath("sponsor");
// plugins/typescript/index.html
fs.mkdirSync("build-website/plugins");
buildForPath("plugins/typescript");
// plugins/json/index.html
buildForPath("plugins/json");

// minify index.css
const styleCssPageFilePath = "build-website/style.css";
const indexCssPageText = fs.readFileSync(styleCssPageFilePath, { encoding: "utf8" });
fs.writeFileSync(styleCssPageFilePath, new CleanCss().minify(indexCssPageText).styles);

// cleanup
fs.unlinkSync(templateHtmlPageFilePath);

/** @param {string} [filePath] - Relative path to the file without extension. */
function buildForPath(filePath) {
    const sponsorMd = fs.readFileSync("docs/" + filePath + ".md", { encoding: "utf8" });
    fs.mkdirSync("build-website/" + filePath);
    fs.writeFileSync("build-website/" + filePath + "/index.html", processMarkdown(sponsorMd));
}

/** @param {string} [mdText] - Markdown to format. */
function processMarkdown(mdText) {
    const innerHtml = converter.makeHtml(mdText);
    return minify(templateHtmlPageText.replace("<!-- inject -->", innerHtml), { collapseWhitespace: true });
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

            throw new Error("Unknown language: " + left);
        }

        function htmlunencode(text) {
            return (text.replace(/&amp;/g, "&")
                .replace(/&lt;/g, "<")
                .replace(/&gt;/g, ">"));
        }
    });
}
