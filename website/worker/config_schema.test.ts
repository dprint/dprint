import { deepStrictEqual, strictEqual, throws } from "node:assert/strict";
import { ConfigTooLargeError, getConfigProperties, getConfigSections, renderConfigHtml } from "./config_schema.ts";

const memberSpacing = {
  description: "The spacing between members.",
  type: "string",
  default: "maintain",
  oneOf: [
    { const: "newLine", description: "Forces a new line." },
    { const: "maintain", description: "Maintains the spacing." },
  ],
};

Deno.test("groups AST node specific properties under the property they refer to", () => {
  const properties = getConfigProperties({
    definitions: {
      memberSpacing,
      "classExpression.memberSpacing": { ...memberSpacing, description: "Class spacing." },
    },
    properties: {
      $schema: { type: "string" },
      locked: { type: "boolean" },
      // declared before its parent, but the parent is still displayed where it's declared
      "enumDeclaration.memberSpacing": { $ref: "#/definitions/memberSpacing" },
      lineWidth: { description: "The width of a line.", type: "number", default: 120 },
      memberSpacing: { $ref: "#/definitions/memberSpacing" },
      "classExpression.memberSpacing": { $ref: "#/definitions/classExpression.memberSpacing" },
    },
  });

  deepStrictEqual(properties.map((p) => p.name), ["lineWidth", "memberSpacing"]);
  deepStrictEqual(properties[1]!.astSpecificProperties, [
    { propertyName: "enumDeclaration.memberSpacing", definition: undefined, hasOwnEntry: false },
    {
      propertyName: "classExpression.memberSpacing",
      definition: { ...memberSpacing, description: "Class spacing." },
      hasOwnEntry: false,
    },
  ]);
});

Deno.test("groups overrides by the schema instead of by their names", () => {
  const sections = getConfigSections({
    "x-sections": [{ title: "General", properties: ["quoteStyle", "spaceAfterKeyword", "jsx.quoteStyle"] }],
    definitions: { quoteStyle: { type: "string", default: "double" } },
    properties: {
      quoteStyle: { $ref: "#/definitions/quoteStyle" },
      // listed, so it's shown on its own too
      "jsx.quoteStyle": { $ref: "#/definitions/quoteStyle", "x-overrides": "quoteStyle" },
      spaceAfterKeyword: { type: "boolean", default: true },
      // its name doesn't say it's an override
      "ifStatement.spaceAfterIf": { type: "boolean", default: true, "x-overrides": "spaceAfterKeyword" },
    },
  });

  const properties = sections[0]!.properties;
  deepStrictEqual(properties.map((p) => [p.name, p.overrides]), [
    ["quoteStyle", undefined],
    ["spaceAfterKeyword", undefined],
    ["jsx.quoteStyle", "quoteStyle"],
  ]);
  deepStrictEqual(properties[0]!.astSpecificProperties, [
    { propertyName: "jsx.quoteStyle", definition: undefined, hasOwnEntry: true },
  ]);
  deepStrictEqual(properties[1]!.astSpecificProperties.map((p) => p.propertyName), ["ifStatement.spaceAfterIf"]);

  const html = renderConfigHtml({ schemaUrl: "https://plugins.dprint.dev/someone/plugin/0.1.0/schema.json", sections });
  strictEqual(
    html.includes(
      "<ul class=\"cfg-chips\"><li><a href=\"#jsx.quoteStyle\"><code>jsx.quoteStyle</code></a></li></ul>",
    ),
    true,
  );
  strictEqual(
    html.includes(
      "<h3 id=\"jsx.quoteStyle\" tabindex=\"-1\"><a class=\"header-anchor\" href=\"#jsx.quoteStyle\">"
        + "<span class=\"cfg-prefix\">jsx.</span>quoteStyle</a></h3>"
        + "<p class=\"cfg-overrides\">Overrides <a href=\"#quoteStyle\"><code>quoteStyle</code></a></p>",
    ),
    true,
  );
});

Deno.test("uses $defs and properties that override the referenced definition", () => {
  const properties = getConfigProperties({
    $defs: { indentWidth: { description: "The indent width.", type: "number", default: 2 } },
    properties: {
      indentWidth: { $ref: "#/$defs/indentWidth", default: 4 },
    },
  });

  strictEqual(properties.length, 1);
  strictEqual(properties[0]!.description, "The indent width.");
  strictEqual(properties[0]!.default, 4);
});

Deno.test("reads the properties under the config key", () => {
  const properties = getConfigProperties({
    properties: {
      cmakefmt: {
        properties: {
          lineWidth: { type: "number", default: 80 },
        },
      },
    },
  }, "cmakefmt");

  deepStrictEqual(properties.map((p) => p.name), ["lineWidth"]);
});

Deno.test("renders descriptions that aren't strings", () => {
  const html = renderConfigHtml({
    schemaUrl: "https://plugins.dprint.dev/someone/plugin/0.1.0/schema.json",
    sections: getConfigSections({
      properties: {
        lineWidth: { description: 5 as unknown as string, type: "number", default: 120 },
      },
    }),
  });

  strictEqual(html.includes("<p>5</p>"), true);
});

Deno.test("renders code spans in descriptions", () => {
  const html = renderConfigHtml({
    schemaUrl: "https://plugins.dprint.dev/someone/plugin/0.1.0/schema.json",
    sections: getConfigSections({
      properties: {
        formatEmbedded: {
          description: "Formats `<style>` and `` css`...` `` templates.",
          type: "boolean",
          default: true,
        },
      },
    }),
  });

  strictEqual(html.includes("<p>Formats <code>&lt;style&gt;</code> and <code>css`...`</code> templates.</p>"), true);
});

Deno.test("renders options that refer to other definitions or are objects", () => {
  const html = renderConfigHtml({
    schemaUrl: "https://plugins.dprint.dev/someone/plugin/0.1.0/schema.json",
    sections: getConfigSections({
      definitions: {
        kind: {
          description: "The kind.",
          oneOf: [{ const: "maintain", description: "Maintains it." }, { const: "sort", description: "Sorts it." }],
        },
      },
      properties: {
        sortNames: {
          default: "maintain",
          oneOf: [{ $ref: "#/definitions/kind" }, {
            type: "object",
            description: "The kind with options.",
            properties: {
              kind: { $ref: "#/definitions/kind" },
              names: { description: "The names.", type: "array", default: [] },
            },
          }],
        },
        tags: { type: "object", default: { css: "less" }, examples: [{ html: "html" }] },
      },
    }),
  });

  strictEqual(
    html.includes(
      "<ul class=\"cfg-values\"><li><code class=\"cfg-value cfg-value-string\">&quot;maintain&quot;</code> "
        + "<span class=\"cfg-default\">Default</span> <span class=\"cfg-value-description\">Maintains it.</span></li>"
        + "<li><code class=\"cfg-value cfg-value-string\">&quot;sort&quot;</code> <span class=\"cfg-value-description\">Sorts it.</span></li>"
        + "<li><code class=\"cfg-value cfg-value-type\">object</code> <span class=\"cfg-value-description\">The kind with options.</span>"
        + "<ul class=\"cfg-object-properties\"><li><code class=\"cfg-object-property\">kind</code> "
        + "<span class=\"cfg-value-description\">The kind.</span>"
        + "<ul class=\"cfg-values\"><li><code class=\"cfg-value cfg-value-string\">&quot;maintain&quot;</code> "
        + "<span class=\"cfg-value-description\">Maintains it.</span></li>"
        + "<li><code class=\"cfg-value cfg-value-string\">&quot;sort&quot;</code> "
        + "<span class=\"cfg-value-description\">Sorts it.</span></li></ul></li>"
        + "<li><code class=\"cfg-object-property\">names</code> <span class=\"cfg-label\">Default</span>"
        + "<code class=\"cfg-value cfg-value-other\">[]</code> <span class=\"cfg-value-description\">The names.</span></li>"
        + "</ul></li></ul>",
    ),
    true,
  );
  strictEqual(
    html.includes(
      "<li><span class=\"cfg-label\">Default</span><code class=\"cfg-value cfg-value-other\">{&quot;css&quot;:&quot;less&quot;}</code></li>",
    ),
    true,
  );
  strictEqual(
    html.includes(
      "<p class=\"cfg-example-label\">Example</p><pre><code class=\"hljs language-json\"><span class=\"hljs-punctuation\">{</span>\n"
        + "  <span class=\"hljs-attr\">&quot;tags&quot;</span>",
    ),
    true,
  );
});

Deno.test("highlights the code examples in the language of their extension", () => {
  const html = renderConfigHtml({
    schemaUrl: "https://plugins.dprint.dev/someone/plugin/0.1.0/schema.json",
    sections: getConfigSections({
      properties: {
        useTabs: {
          type: "boolean",
          default: false,
          oneOf: [{ const: true }, { const: false }],
          "x-example": {
            extension: "ts",
            input: ["const a = 'b'"],
            outputs: { true: "const a = \"b\";\n", false: "const a = \"b\";\n" },
          },
        },
        semicolons: {
          type: "boolean",
          default: false,
          oneOf: [{ const: true }, { const: false }],
          // an unknown language isn't highlighted
          "x-example": { extension: "unknown", input: ["a<b"], outputs: { true: "a<b;\n", false: "a<b\n" } },
        },
      },
    }),
  });

  strictEqual(
    html.includes(
      "<pre><code class=\"hljs language-typescript\"><span class=\"hljs-keyword\">const</span> a = "
        + "<span class=\"hljs-string\">&#x27;b&#x27;</span></code></pre>",
    ),
    true,
  );
  strictEqual(html.includes("<pre><code>a&lt;b;</code></pre>"), true);
});

Deno.test("renders the code examples of each value", () => {
  const html = renderConfigHtml({
    schemaUrl: "https://plugins.dprint.dev/someone/plugin/0.1.0/schema.json",
    sections: getConfigSections({
      definitions: {
        semicolons: {
          default: "always",
          oneOf: [
            { const: "always" },
            { const: "semiColon", deprecationMessage: "Use \"always\" instead." },
            { const: "asi" },
          ],
        },
      },
      properties: {
        semicolons: {
          $ref: "#/definitions/semicolons",
          "x-example": {
            config: { lineWidth: 40 },
            input: ["const a = 1", "const b = 2"],
            outputs: { always: "const a = 1;\nconst b = 2;\n", asi: "const a = 1\nconst b = 2\n" },
          },
        },
        useTabs: {
          type: "boolean",
          default: false,
          oneOf: [{ const: true }, { const: false }],
          "x-example": { input: ["{", "  a;", "}"], outputs: { true: "{\n\ta;\n}\n", false: "{\n  a;\n}\n" } },
        },
      },
    }),
  });

  strictEqual(
    html.includes(
      "<p class=\"cfg-example-label\">Each value below shows how it formats this code with "
        + "<code>&quot;lineWidth&quot;: 40</code></p>"
        + "<pre><code>const a = 1\nconst b = 2</code></pre>"
        + "<ul class=\"cfg-values\"><li><code class=\"cfg-value cfg-value-string\">&quot;always&quot;</code> "
        + "<span class=\"cfg-default\">Default</span><pre><code>const a = 1;\nconst b = 2;</code></pre></li>"
        + "<li><code class=\"cfg-value cfg-value-string\">&quot;asi&quot;</code><pre><code>const a = 1\nconst b = 2</code></pre></li></ul>",
    ),
    true,
  );
  strictEqual(
    html.includes(
      "<p class=\"cfg-example-label\">Each value below shows how it formats this code</p><pre><code>{\n  a;\n}</code></pre>"
        + "<ul class=\"cfg-values\"><li><code class=\"cfg-value cfg-value-boolean\">true</code><pre><code>{\n\ta;\n}</code></pre></li>"
        + "<li><code class=\"cfg-value cfg-value-boolean\">false</code> <span class=\"cfg-default\">Default</span>"
        + "<pre><code>{\n  a;\n}</code></pre></li></ul>",
    ),
    true,
  );
});

Deno.test("groups properties into sections with the rest in an other section", () => {
  const sections = getConfigSections({
    "x-sections": [
      { title: "General", description: "All `code`.", properties: ["lineWidth", "missing", "useTabs"] },
      { title: "Spaces", properties: ["useTabs", "spaceAround"] },
    ],
    properties: {
      useTabs: { type: "boolean", default: false },
      spaceAround: { type: "boolean", default: false },
      lineWidth: { type: "integer", default: 120 },
      quoteStyle: { type: "string", default: "double" },
    },
  });

  deepStrictEqual(sections.map((s) => [s.title, s.properties.map((p) => p.name)]), [
    ["General", ["lineWidth", "useTabs"]],
    ["Spaces", ["spaceAround"]],
    ["Other", ["quoteStyle"]],
  ]);

  const html = renderConfigHtml({ schemaUrl: "https://plugins.dprint.dev/someone/plugin/0.1.0/schema.json", sections });
  strictEqual(
    html.includes(
      "<ul class=\"cfg-toc\"><li><a href=\"#general\">General<span class=\"cfg-count\">2</span></a></li>"
        + "<li><a href=\"#spaces\">Spaces<span class=\"cfg-count\">1</span></a></li>"
        + "<li><a href=\"#other\">Other<span class=\"cfg-count\">1</span></a></li></ul>"
        + "<h2 id=\"general\" tabindex=\"-1\"><a class=\"header-anchor\" href=\"#general\">General</a></h2>"
        + "<p>All <code>code</code>.</p>"
        + "<div class=\"cfg-property\"><h3 id=\"lineWidth\" tabindex=\"-1\"><a class=\"header-anchor\" href=\"#lineWidth\">lineWidth</a></h3>",
    ),
    true,
  );
});

Deno.test("leaves out a default that isn't specified", () => {
  const html = renderConfigHtml({
    schemaUrl: "https://plugins.dprint.dev/someone/plugin/0.1.0/schema.json",
    sections: getConfigSections({ properties: { blankLines: { type: "integer" } } }),
  });

  strictEqual(
    html.includes(
      "<ul class=\"cfg-meta\"><li><span class=\"cfg-label\">Type</span><code class=\"cfg-value cfg-value-type\">integer</code></li></ul>",
    ),
    true,
  );
});

Deno.test("renders properties", () => {
  const html = renderConfigHtml({
    schemaUrl: "https://plugins.dprint.dev/dprint/dprint-plugin-test/0.1.0/schema.json",
    sections: getConfigSections({
      definitions: { memberSpacing },
      properties: {
        lineWidth: { description: "Uses <width> & stuff.", type: "number", default: 120 },
        memberSpacing: { $ref: "#/definitions/memberSpacing" },
        "enumDeclaration.memberSpacing": { $ref: "#/definitions/memberSpacing" },
        preferSingleLine: { type: "boolean", enum: [true, false], default: false },
      },
    }),
  });

  strictEqual(
    html,
    "<p>This information was auto generated from <a href=\"https://plugins.dprint.dev/dprint/dprint-plugin-test/0.1.0/schema.json\">https://plugins.dprint.dev/dprint/dprint-plugin-test/0.1.0/schema.json</a>.</p>"
      + "<div class=\"cfg-property\"><h2 id=\"lineWidth\" tabindex=\"-1\"><a class=\"header-anchor\" href=\"#lineWidth\">lineWidth</a></h2>"
      + "<p>Uses &lt;width&gt; &amp; stuff.</p>"
      + "<ul class=\"cfg-meta\"><li><span class=\"cfg-label\">Type</span><code class=\"cfg-value cfg-value-type\">number</code></li>"
      + "<li><span class=\"cfg-label\">Default</span><code class=\"cfg-value cfg-value-number\">120</code></li></ul></div>"
      + "<div class=\"cfg-property\"><h2 id=\"memberSpacing\" tabindex=\"-1\"><a class=\"header-anchor\" href=\"#memberSpacing\">memberSpacing</a></h2>"
      + "<p>The spacing between members.</p>"
      + "<ul class=\"cfg-values\"><li><code class=\"cfg-value cfg-value-string\">&quot;newLine&quot;</code> "
      + "<span class=\"cfg-value-description\">Forces a new line.</span></li>"
      + "<li><code class=\"cfg-value cfg-value-string\">&quot;maintain&quot;</code> <span class=\"cfg-default\">Default</span> "
      + "<span class=\"cfg-value-description\">Maintains the spacing.</span></li></ul>"
      + "<details class=\"cfg-node-overrides\"><summary>1 AST node specific property</summary>"
      + "<ul class=\"cfg-chips\"><li id=\"enumDeclaration.memberSpacing\"><code>enumDeclaration.memberSpacing</code></li></ul></details></div>"
      + "<div class=\"cfg-property\"><h2 id=\"preferSingleLine\" tabindex=\"-1\"><a class=\"header-anchor\" href=\"#preferSingleLine\">preferSingleLine</a></h2>"
      + "<ul class=\"cfg-values\"><li><code class=\"cfg-value cfg-value-boolean\">true</code></li>"
      + "<li><code class=\"cfg-value cfg-value-boolean\">false</code> <span class=\"cfg-default\">Default</span></li></ul></div>",
  );
});

Deno.test("expands a definition that refers to itself once", () => {
  const sections = getConfigSections({
    definitions: { mode: { oneOf: Array.from({ length: 50 }, () => ({ $ref: "#/definitions/mode" })) } },
    properties: { mode: { $ref: "#/definitions/mode" } },
  });
  const html = renderConfigHtml({ schemaUrl: "https://plugins.dprint.dev/someone/plugin/0.1.0/schema.json", sections });

  strictEqual(sections[0]!.properties[0]!.oneOf!.length <= 200, true);
  strictEqual(html.length < 50_000, true);
});

Deno.test("stops rendering a configuration that's too large", () => {
  const output = "const a = 1;\n".repeat(2_000);
  const sections = getConfigSections({
    properties: {
      mode: { enum: Array(200).fill("a"), "x-example": { extension: "ts", input: ["a"], outputs: { a: output } } },
    },
  });

  throws(
    () => renderConfigHtml({ schemaUrl: "https://plugins.dprint.dev/someone/plugin/0.1.0/schema.json", sections }),
    ConfigTooLargeError,
  );
});

Deno.test("renders names that are on the object prototype", () => {
  const html = renderConfigHtml({
    schemaUrl: "https://plugins.dprint.dev/someone/plugin/0.1.0/schema.json",
    sections: getConfigSections({
      properties: {
        "node.constructor": { $ref: "#/definitions/constructor" },
        mode: {
          enum: ["toString", "__proto__"],
          "x-example": { extension: "constructor", input: ["a"], outputs: { toString: "b" } },
        },
      },
    }),
  });

  strictEqual(html.includes("<pre><code>b</code></pre>"), true);
  strictEqual(html.includes("node.constructor"), true);
});
