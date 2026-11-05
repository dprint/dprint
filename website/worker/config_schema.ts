// renders a plugin's configuration documentation from its schema.json

import { highlightCode } from "./highlight.ts";

/** A JSON schema definition for a configuration property. */
export interface SchemaDefinition {
  description?: string;
  type?: string | string[];
  default?: unknown;
  const?: unknown;
  oneOf?: SchemaDefinition[];
  anyOf?: SchemaDefinition[];
  enum?: unknown[];
  /** The properties of an object value. */
  properties?: Record<string, SchemaDefinition>;
  /** The definition of the items of an array value. */
  items?: SchemaDefinition;
  examples?: unknown[];
  /** Set on a value that's still accepted, but no longer documented. */
  deprecationMessage?: string;
  deprecated?: boolean;
  "x-example"?: CodeExample;
  /** The property this property overrides for one kind of node. */
  "x-overrides"?: string;
  $ref?: string;
}

/**
 * Code formatted with each value of a property, which a plugin generates when
 * it publishes its schema.
 */
export interface CodeExample {
  /** The configuration the code was formatted with, besides the property. */
  config?: Record<string, unknown>;
  /** The file extension of the code, which sets the language it's highlighted as. */
  extension?: string;
  input: string[];
  /** The formatted code by value, where a value that isn't a string is in JSON. */
  outputs?: Record<string, string>;
}

interface RootSchema {
  properties: Record<string, SchemaDefinition>;
  definitions?: Record<string, SchemaDefinition>;
  $defs?: Record<string, SchemaDefinition>;
  /** The sections to show the properties in, in order. */
  "x-sections"?: SchemaSection[];
}

interface SchemaSection {
  title: string;
  description?: string;
  /** The top level properties in the section, in order. */
  properties: string[];
}

/** A section of the configuration documentation. */
export interface ConfigSection {
  /** Not set when the schema doesn't have sections. */
  title: string | undefined;
  description: string | undefined;
  properties: ConfigProperty[];
}

/** A top level configuration property along with its AST node specific overrides. */
export interface ConfigProperty extends SchemaDefinition {
  name: string;
  /** The property this property overrides, when it's shown on its own. */
  overrides?: string;
  astSpecificProperties: AstSpecificProperty[];
}

export interface AstSpecificProperty {
  propertyName: string;
  // only set when it differs from the parent property's definition and isn't
  // shown on its own
  definition: SchemaDefinition | undefined;
  /** Whether it's also shown on its own because it's listed in a section. */
  hasOwnEntry: boolean;
}

export interface RenderConfigOptions {
  schemaUrl: string;
  sections: ConfigSection[];
}

// properties every plugin schema may have that aren't plugin configuration
const IGNORED_PROPERTY_NAMES = new Set(["$schema", "deno", "locked"]);
// the most values shown for a property, since a small schema can describe many
// (ex. options that refer to definitions with options)
const MAX_OPTIONS = 200;
// the most text a page's configuration has, which is about ten times
// dprint-plugin-typescript's with its code examples. a small schema can describe
// a lot of text (ex. the same code example for many values)
const MAX_RENDERED_LENGTH = 1_500_000;
// longer code isn't highlighted, which takes too long
const MAX_HIGHLIGHTED_LENGTH = 20_000;

/** Thrown when a schema's configuration is too large to render. */
export class ConfigTooLargeError extends Error {}

// the text left before the page is too large, which is only set while rendering
let remainingRenderedLength: number | undefined;

/**
 * Gets the configuration properties from a plugin's schema.json grouped into
 * the sections in its `x-sections`.
 *
 * A property that isn't in a section goes in an "Other" section at the end.
 * When the schema doesn't have sections, all the properties are in one section
 * without a title.
 */
export function getConfigSections(rootSchema: unknown, configKey?: string): ConfigSection[] {
  const properties = getConfigProperties(rootSchema, configKey);
  const schemaSections = getSchema(rootSchema, configKey)["x-sections"];
  if (schemaSections == null) {
    return [{ title: undefined, description: undefined, properties }];
  }

  const remainingProperties = new Map(properties.map((property) => [property.name, property]));
  const sections: ConfigSection[] = schemaSections.map((section) => ({
    title: section.title,
    description: section.description,
    properties: section.properties.flatMap((name) => {
      const property = remainingProperties.get(name);
      // a property that doesn't exist or is in an earlier section is skipped
      if (property == null) {
        return [];
      }
      remainingProperties.delete(name);
      return [property];
    }),
  }));
  if (remainingProperties.size > 0) {
    sections.push({ title: "Other", description: undefined, properties: [...remainingProperties.values()] });
  }
  return sections;
}

/**
 * Gets the configuration properties from a plugin's schema.json.
 *
 * Properties that refer to the definition of another property (ex.
 * `classExpression.memberSpacing` referring to `memberSpacing`) are grouped
 * under that property as AST node specific properties.
 */
export function getConfigProperties(rootSchema: unknown, configKey?: string): ConfigProperty[] {
  const root = rootSchema as RootSchema;
  const schema = getSchema(rootSchema, configKey);
  const definitions = root.definitions ?? root.$defs ?? {};
  const properties = new Map<string, ConfigProperty>();
  // a property is displayed where it's declared, which may be after the AST
  // node specific properties that caused it to be created
  const declarationOrder = new Map<string, number>();

  // the schema says which property an override is under, which is otherwise
  // guessed from the property names
  const usesOverrides = Object.values(schema.properties).some((property) => property["x-overrides"] != null);
  const listedPropertyNames = new Set(schema["x-sections"]?.flatMap((section) => section.properties) ?? []);
  const resolvedDefinitions = new Map<string, SchemaDefinition>();
  const resolvingDefinitionNames = new Set<string>();
  // the names come from the schema, so they're looked up without the prototype
  // (ex. `constructor`)
  const isProperty = (name: string) => Object.hasOwn(schema.properties, name);

  for (const [propertyName, property] of Object.entries(schema.properties)) {
    if (IGNORED_PROPERTY_NAMES.has(propertyName)) {
      continue;
    }

    // a property may specify its own default alongside the reference
    const definition = resolveRef(property);
    const parentPropertyName = usesOverrides
      ? property["x-overrides"]
      : getGuessedParentPropertyName(propertyName, property);
    if (parentPropertyName == null || !isProperty(parentPropertyName)) {
      declareProperty(propertyName, definition);
      continue;
    }

    const hasOwnEntry = usesOverrides && listedPropertyNames.has(propertyName);
    const isSameDefinition = property.$ref != null && property.$ref === schema.properties[parentPropertyName]!.$ref;
    getOrCreateProperty(parentPropertyName).astSpecificProperties.push({
      propertyName,
      definition: isSameDefinition || hasOwnEntry ? undefined : resolveOptions(definition),
      hasOwnEntry,
    });
    if (hasOwnEntry) {
      declareProperty(propertyName, definition);
      getOrCreateProperty(propertyName).overrides = parentPropertyName;
    }
  }

  return [...properties.values()].sort((a, b) => getOrder(a) - getOrder(b));

  function declareProperty(propertyName: string, definition: SchemaDefinition) {
    Object.assign(getOrCreateProperty(propertyName), resolveOptions(definition));
    declarationOrder.set(propertyName, declarationOrder.size);
  }

  // resolves the references in a definition's options and object properties, and
  // replaces an option that refers to a definition with options with those options
  // (ex. a kind of sorting or an object with the kind and its options)
  function resolveOptions(definition: SchemaDefinition): SchemaDefinition {
    const resolved = { ...definition };
    // schema generators often use anyOf for an optional value (ex. a mode or null)
    const options = definition.oneOf ?? definition.anyOf;
    if (options != null) {
      resolved.oneOf = [];
      addOptions: for (const option of options) {
        for (const value of getOptionValues(resolveDefinition(option))) {
          // a schema can describe more options than a configuration has
          if (resolved.oneOf.length >= MAX_OPTIONS) {
            break addOptions;
          }
          resolved.oneOf.push(value);
        }
      }
    }
    if (definition.properties != null) {
      resolved.properties = Object.fromEntries(
        Object.entries(definition.properties).map(([name, property]) => [name, resolveDefinition(property)]),
      );
    }
    if (definition.items != null) {
      resolved.items = resolveDefinition(definition.items);
    }
    return resolved;
  }

  // resolves each referenced definition once, because a schema can refer to one
  // many times, and doesn't expand a definition within itself
  function resolveDefinition(definition: SchemaDefinition): SchemaDefinition {
    if (definition.$ref == null) {
      return resolveOptions(definition);
    }
    const { $ref, ...rest } = definition;
    const name = getDefinitionName($ref);
    let referenced = resolvedDefinitions.get(name);
    if (referenced == null) {
      const unresolved = Object.hasOwn(definitions, name) ? definitions[name]! : {};
      if (resolvingDefinitionNames.has(name)) {
        return { ...unresolved, ...rest };
      }
      resolvingDefinitionNames.add(name);
      referenced = resolveOptions(unresolved);
      resolvingDefinitionNames.delete(name);
      resolvedDefinitions.set(name, referenced);
    }
    return { ...referenced, ...rest };
  }

  // an option is shown as its values when it has a list of them
  function getOptionValues(option: SchemaDefinition): SchemaDefinition[] {
    if (option.const !== undefined) {
      return [option];
    } else if (option.oneOf != null) {
      return option.oneOf;
    } else if (option.enum != null) {
      return option.enum.map((value) => ({ const: value }));
    } else if (option.type === "null") {
      return [{ const: null, description: option.description }];
    } else if (option.type === "boolean") {
      return [{ const: true, description: option.description }, { const: false, description: option.description }];
    } else {
      return [option];
    }
  }

  function resolveRef(definition: SchemaDefinition): SchemaDefinition {
    if (definition.$ref == null) {
      return definition;
    }
    const { $ref, ...rest } = definition;
    const name = getDefinitionName($ref);
    return { ...(Object.hasOwn(definitions, name) ? definitions[name] : undefined), ...rest };
  }

  function getDefinitionName($ref: string) {
    return $ref.replace(/^#\/(definitions|\$defs)\//, "");
  }

  function getOrder(property: ConfigProperty) {
    return declarationOrder.get(property.name) ?? Number.MAX_SAFE_INTEGER;
  }

  // a property that refers to another property's definition or whose last
  // segment is another property's name (ex. `classExpression.memberSpacing`
  // with `memberSpacing`) is likely an override of it
  function getGuessedParentPropertyName(propertyName: string, property: SchemaDefinition) {
    if (property.$ref == null) {
      return undefined;
    }
    const derivedPropName = property.$ref.replace(/^#\/(definitions|\$defs)\//, "");
    const lastSegment = propertyName.split(".").pop()!;
    if (derivedPropName !== propertyName && isProperty(derivedPropName)) {
      return derivedPropName;
    } else if (lastSegment !== propertyName && isProperty(lastSegment)) {
      return lastSegment;
    } else {
      return undefined;
    }
  }

  function getOrCreateProperty(propertyName: string) {
    let property = properties.get(propertyName);
    if (property == null) {
      property = { name: propertyName, astSpecificProperties: [] };
      properties.set(propertyName, property);
    }
    return property;
  }
}

/** Renders the HTML that goes inside a page's `.plugin-config-table` element. */
export function renderConfigHtml(options: RenderConfigOptions) {
  remainingRenderedLength = MAX_RENDERED_LENGTH;
  try {
    return renderConfigHtmlWithinLimit(options);
  } finally {
    remainingRenderedLength = undefined;
  }
}

function renderConfigHtmlWithinLimit(options: RenderConfigOptions) {
  const { schemaUrl, sections } = options;
  let html = `<p>This information was auto generated from <a href="${escapeHtml(schemaUrl)}">${escapeHtml(schemaUrl)}</a>.</p>`;
  const hasTitles = sections.some((section) => section.title != null);
  if (!hasTitles) {
    return html + sections.map((section) => renderProperties(section.properties, 2)).join("");
  }

  // a table of contents, then each section with its properties a level below it
  html += `<ul class="cfg-toc">`;
  for (const section of sections) {
    html += `<li><a href="#${getSectionId(section)}">${escapeHtml(section.title)}`
      + `<span class="cfg-count">${section.properties.length}</span></a></li>`;
  }
  html += "</ul>";
  for (const section of sections) {
    html += renderHeading(getSectionId(section), escapeHtml(section.title), 2);
    html += renderDescription(section);
    html += renderProperties(section.properties, 3);
  }
  return html;
}

/** Renders the HTML shown when the configuration couldn't be rendered. */
export function renderConfigErrorHtml(message: string, link: { href: string; text: string }) {
  return `<p>${escapeHtml(message)} <a href="${escapeHtml(link.href)}">${escapeHtml(link.text)}</a></p>`;
}

// takes any value because third party schemas may not use strings where they should
export function escapeHtml(text: unknown) {
  return countRenderedText(
    String(text)
      .replaceAll("&", "&amp;")
      .replaceAll("<", "&lt;")
      .replaceAll(">", "&gt;")
      .replaceAll("\"", "&quot;")
      .replaceAll("'", "&#39;"),
  );
}

// all the text from a schema is escaped or highlighted, so counting it there
// limits the size of the page
function countRenderedText(text: string) {
  if (remainingRenderedLength != null) {
    remainingRenderedLength -= text.length;
    if (remainingRenderedLength < 0) {
      throw new ConfigTooLargeError(`The configuration is over ${MAX_RENDERED_LENGTH} characters.`);
    }
  }
  return text;
}

// some schemas nest the plugin config under its config key
function getSchema(rootSchema: unknown, configKey: string | undefined) {
  const root = rootSchema as RootSchema;
  return configKey == null ? root : root.properties[configKey] as unknown as RootSchema;
}

function getSectionId(section: ConfigSection) {
  return String(section.title).toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
}

function renderProperties(properties: ConfigProperty[], headingLevel: number) {
  let html = "";
  for (const property of properties) {
    html += `<div class="cfg-property">`;
    html += renderHeading(property.name, renderPropertyName(property.name), headingLevel);
    if (property.overrides != null) {
      const overrides = escapeHtml(property.overrides);
      html += `<p class="cfg-overrides">Overrides <a href="#${overrides}"><code>${overrides}</code></a></p>`;
    }
    html += renderDescription(property);
    html += renderInfo(property);
    html += renderExamples(property);
    html += renderAstSpecificProperties(property);
    html += "</div>";
  }
  return html;
}

// collapsed because a property can have dozens, and the site's script opens it
// when linked to one of them
function renderAstSpecificProperties(property: ConfigProperty) {
  const count = property.astSpecificProperties.length;
  if (count === 0) {
    return "";
  }
  let html = `<details class="cfg-node-overrides"><summary>${count} AST node specific `
    + `${count === 1 ? "property" : "properties"}</summary>`;
  // the ones that differ from the property are described below the rest
  const sameProperties = property.astSpecificProperties.filter((p) => p.definition == null);
  const differentProperties = property.astSpecificProperties.filter((p) => p.definition != null);
  if (sameProperties.length > 0) {
    html += `<ul class="cfg-chips">`;
    for (const { propertyName, hasOwnEntry } of sameProperties) {
      const name = escapeHtml(propertyName);
      // one with its own entry links to it, which has the id
      html += hasOwnEntry
        ? `<li><a href="#${name}"><code>${name}</code></a></li>`
        : `<li id="${name}"><code>${name}</code></li>`;
    }
    html += "</ul>";
  }
  for (const { propertyName, definition } of differentProperties) {
    html += `<div class="cfg-node-override" id="${escapeHtml(propertyName)}"><p><code>${escapeHtml(propertyName)}</code></p>`;
    if (definition!.description !== property.description) {
      html += renderDescription(definition!);
    }
    html += renderInfo(definition!);
    html += "</div>";
  }
  return html + "</details>";
}

// dims the kind of node of an AST node specific property (ex. `classDeclaration.`)
function renderPropertyName(name: string) {
  const dotIndex = name.lastIndexOf(".");
  if (dotIndex === -1) {
    return escapeHtml(name);
  }
  return `<span class="cfg-prefix">${escapeHtml(name.slice(0, dotIndex + 1))}</span>${escapeHtml(name.slice(dotIndex + 1))}`;
}

// matches the headings markdown-it-anchor generates for the rest of the site,
// except the id of a property is its name so links like `#indentWidth` work
function renderHeading(id: string, textHtml: string, level: number) {
  const escapedId = escapeHtml(id);
  return `<h${level} id="${escapedId}" tabindex="-1"><a class="header-anchor" href="#${escapedId}">${textHtml}</a></h${level}>`;
}

function renderDescription(definition: { description?: string }) {
  return definition.description == null ? "" : `<p>${renderText(definition.description)}</p>`;
}

function renderInfo(definition: SchemaDefinition) {
  let html = renderExampleInput(definition);
  if (definition.oneOf != null) {
    html += `<ul class="cfg-values">${renderOptions(definition)}</ul>`;
  } else if (definition.enum != null) {
    html += `<ul class="cfg-values">`;
    for (const value of definition.enum) {
      html += `<li>${renderValue(value)}`;
      if (value === definition.default) {
        html += renderDefaultBadge();
      }
      html += renderExampleOutput(definition, value);
      html += "</li>";
    }
    html += "</ul>";
  } else {
    html += `<ul class="cfg-meta">`;
    if (definition.type != null) {
      html += `<li><span class="cfg-label">Type</span>${renderType(definition)}</li>`;
    }
    if (definition.default !== undefined) {
      html += `<li><span class="cfg-label">Default</span>${renderValue(definition.default)}</li>`;
    }
    html += "</ul>";
  }
  if (definition.items?.properties != null) {
    html += "<p>Each item is an object with these properties:</p>";
    html += renderObjectProperties(definition.items.properties);
  }
  return html;
}

function renderOptions(definition: SchemaDefinition) {
  let html = "";
  for (const option of definition.oneOf ?? []) {
    if (option.deprecationMessage != null || option.deprecated === true) {
      continue;
    }
    // an option that isn't a single value is described by its type (ex. an object)
    const isValue = option.const !== undefined;
    html += "<li>";
    html += isValue ? renderValue(option.const) : renderType(option);
    if (isValue && option.const === definition.default) {
      html += renderDefaultBadge();
    }
    if (option.description != null && option.description.length > 0) {
      html += ` <span class="cfg-value-description">${renderText(option.description)}</span>`;
    }
    if (option.properties != null) {
      html += renderObjectProperties(option.properties);
    }
    if (isValue) {
      html += renderExampleOutput(definition, option.const);
    }
    html += "</li>";
  }
  return html;
}

function renderExampleInput(definition: SchemaDefinition) {
  const example = definition["x-example"];
  if (example?.outputs == null) {
    return "";
  }
  const config = Object.entries(example.config ?? {})
    .map(([name, value]) => `${JSON.stringify(name)}: ${JSON.stringify(value)}`)
    .join(", ");
  const withConfig = config.length === 0 ? "" : ` with <code>${escapeHtml(config)}</code>`;
  return `<p class="cfg-example-label">Each value below shows how it formats this code${withConfig}</p>`
    + renderCode(example.input.join("\n"), example.extension);
}

function renderExampleOutput(definition: SchemaDefinition, value: unknown) {
  const example = definition["x-example"];
  const key = typeof value === "string" ? value : JSON.stringify(value);
  // the key comes from the schema, so it's looked up without the prototype (ex. `constructor`)
  const output = example?.outputs != null && Object.hasOwn(example.outputs, key) ? example.outputs[key] : undefined;
  return typeof output === "string" ? renderCode(output.trimEnd(), example?.extension) : "";
}

// highlights the code when the extension is of a known language
function renderCode(code: string, extension: string | undefined) {
  // counted before highlighting, so a page that's too large doesn't highlight
  // the code that makes it too large
  countRenderedText(code);
  const highlighted = code.length > MAX_HIGHLIGHTED_LENGTH ? undefined : highlightCode(code, extension);
  if (highlighted == null) {
    return `<pre><code>${escapeHtml(code)}</code></pre>`;
  }
  return `<pre><code class="hljs language-${highlighted.language}">${countRenderedText(highlighted.html)}</code></pre>`;
}

function renderObjectProperties(properties: Record<string, SchemaDefinition>) {
  let html = `<ul class="cfg-object-properties">`;
  for (const [name, property] of Object.entries(properties)) {
    html += `<li><code class="cfg-object-property">${escapeHtml(name)}</code>`;
    if (property.default !== undefined) {
      html += ` <span class="cfg-label">Default</span>${renderValue(property.default)}`;
    }
    if (property.description != null && property.description.length > 0) {
      html += ` <span class="cfg-value-description">${renderText(property.description)}</span>`;
    }
    if (property.oneOf != null) {
      html += `<ul class="cfg-values">${renderOptions(property)}</ul>`;
    }
    html += "</li>";
  }
  return html + "</ul>";
}

function renderExamples(property: ConfigProperty) {
  let html = "";
  for (const example of property.examples ?? []) {
    const json = JSON.stringify({ [property.name]: example }, undefined, 2);
    html += `<p class="cfg-example-label">Example</p>${renderCode(json, "json")}`;
  }
  return html;
}

// colored like the value is in the highlighted code
function renderValue(value: unknown) {
  const kind = typeof value === "string" || typeof value === "number" || typeof value === "boolean"
    ? typeof value
    : "other";
  return `<code class="cfg-value cfg-value-${kind}">${escapeHtml(valueToText(value))}</code>`;
}

function renderType(definition: SchemaDefinition) {
  return `<code class="cfg-value cfg-value-type">${escapeHtml(getTypeText(definition))}</code>`;
}

// includes the type of an array's items (ex. `string[]`)
function getTypeText(definition: SchemaDefinition): string {
  if (definition.type === "array" && definition.items != null) {
    const itemType = getTypeText(definition.items);
    return itemType.includes(" ") ? `(${itemType})[]` : `${itemType}[]`;
  }
  if (Array.isArray(definition.type)) {
    return definition.type.join(" | ");
  }
  return definition.type ?? (definition.properties != null ? "object" : "any");
}

function renderDefaultBadge() {
  return ` <span class="cfg-default">Default</span>`;
}

// escapes the text and renders its markdown code spans, which are surrounded by
// more backticks when the code has one (ex. `` html`...` ``)
function renderText(text: unknown) {
  return escapeHtml(text).replace(/(?<!`)(`+)(?!`)(.+?)(?<!`)\1(?!`)/g, (_match, _backticks, code: string) => {
    const isPadded = code.length > 2 && code.startsWith(" ") && code.endsWith(" ");
    return `<code>${isPadded ? code.slice(1, -1) : code}</code>`;
  });
}

function valueToText(value: unknown) {
  if (typeof value === "string") {
    return `"${value}"`;
  }
  if (value === null) {
    return "null";
  }
  if (value === undefined) {
    return "<not specified>";
  }
  if (typeof value === "object") {
    return JSON.stringify(value);
  }
  return String(value);
}
