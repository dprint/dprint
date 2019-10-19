// @ts-check
const showdown = require("showdown");
const hljs = require("highlight.js");
const fs = require("fs");
const { minify } = require("html-minifier");
const CleanCss = require("clean-css");

// update index.html
initCodeHighlightExtension();

const converter = new showdown.Converter({ extensions: ["codehighlight"] });
converter.setFlavor("github");
const markdownAsHtml = converter.makeHtml(fs.readFileSync("docs/home.md", { encoding: "utf8" }));

const indexPageFilePath = "build-website/index.html";
const indexPageText = fs.readFileSync(indexPageFilePath, { encoding: "utf8" });
fs.writeFileSync(indexPageFilePath, minify(indexPageText.replace("<!-- inject -->", markdownAsHtml), { collapseWhitespace: true }));

// minify index.css
const indexCssPageFilePath = "build-website/index.css";
const indexCssPageText = fs.readFileSync(indexCssPageFilePath, { encoding: "utf8" });
fs.writeFileSync(indexCssPageFilePath, new CleanCss().minify(indexCssPageText).styles);

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
                    return left + hljs.highlightAuto(match).value + right;
                };
                return showdown.helper.replaceRecursiveRegExp(text, replacement, left, right, flags);
            }
        }];

        function htmlunencode(text) {
            return (text.replace(/&amp;/g, "&")
                .replace(/&lt;/g, "<")
                .replace(/&gt;/g, ">"));
        }
    });
}
