*dprint* is a configurable and plugable code formatter.

Currently only TypeScript and JSONC is supported.

## Install

Install *dprint* and the plugins you want to use as a dev dependency.

For example:

```
yarn add --dev dprint dprint-plugin-typescript dprint-plugin-jsonc
# or
npm install --save-dev dprint dprint-plugin-typescript dprint-plugin-jsonc
```

## Setup and Usage

Create a *dprint.config.js* file in the repo. Here's an example (you don't need to copy this—use your own config):

```js
// @ts-check
const { TypeScriptPlugin } = require("dprint-plugin-typescript");
const { JsoncPlugin } = require("dprint-plugin-jsonc");

/** @type { import("dprint").Configuration } */
module.exports.config = {
    projectType: "openSource",
    lineWidth: 160,
    plugins: [
        new TypeScriptPlugin({
            useBraces: "preferNone",
            "tryStatement.nextControlFlowPosition": "sameLine"
        }),
        new JsoncPlugin({
            indentWidth: 2,
            lineWidth: 80
        })
    ]
};
```

Add a format script to your *package.json*'s "scripts" section (see `npx dprint --help` for usage):

```ts
{
  "name": "your-package-name",
  "scripts": {
    "format": "dprint \"**/*{.ts,.tsx,.json,.js}\""
  }
}
```

Format:

```
yarn format
# or
npm run format
```

## Global Configuration

There are certain non-language specific configuration that can be specified. Note though, that it is always possible to override these settings on a per-language basis (with the exception of `projectType`). For example:

```ts
module.exports.config = {
    projectType: "openSource",
    lineWidth: 160,
    useTabs: true,
    plugins: [
        new TypeScriptPlugin({}),
        new JsoncPlugin({
            lineWidth: 80,
            indentWidth: 2,
            useTabs: false,
        })
    ]
};
```

### `projectType`

This is required when using the cli. Specify the type of project dprint is formatting. You may specify any of the allowed values here according to your conscience.

* `"openSource"` - Dprint is formatting an open source project.
* `"commercialSponsored"` - Dprint is formatting a closed source commercial project and your company sponsored dprint.
* `"commercialDidNotSponsor"` - Dprint is formatting a closed source commercial project and you want to forever enshrine your name in source control for having specified this.

### `lineWidth`

**Type:** `number`
**Default:** `120`

The width of a line the printer will try to stay under. Note that the printer may exceed this width in certain cases.

### `indentWidth`

**Type:** `number`
**Default:** `4`

The number of spaces for an indent. This option is ignored when using tabs.

### `useTabs`

**Type:** `boolean`
**Default:** `false`

Whether to use tabs (`true`) or spaces (`false`).

## TypeScript

Install plugin via:

```
yarn add --dev dprint-plugin-typescript
# or
npm install --save-dev dprint-plugin-typescript
```

Then add it to the configuration in *dprint.config.js*:

```js
// @ts-check
const { TypeScriptPlugin } = require("dprint-plugin-typescript");

/** @type { import("dprint").Configuration } */
module.exports.config = {
    projectType: "openSource",
    plugins: [
        new TypeScriptPlugin({})
    ]
};
```

### Node Specific Configuration

Where applicable and in most situations, configuration can be set for specific kinds of declarations and statements.

For example, you can specify the general `nextControlFlowPosition`, but you can also specify a more specific `"ifStatement.nextControlFlowPosition"` option that will be used for that statement.

```ts
module.exports.config = {
    projectType: "openSource",
    plugins: [
        new TypeScriptPlugin({
            nextControlFlowPosition: "maintain",
            "ifStatement.nextControlFlowPosition": "sameLine",
            "returnStatement.semiColon": false
        })
    ]
};
```

### `semiColons`

**Type:** `boolean`
**Default:** `true`

Whether to use semi-colons are not.

Note that when `semiColons` is `false` (or more specifically, when `"expressionStatement.semiColon"` is `false`), it will insert semi-colons at the beginning of some statements. Read why this is done here: https://standardjs.com/rules.html#semicolons

### `singleQuotes`

**Type:** `boolean`
**Default:** `false`

Whether to use single quotes (`true`) or double quotes (`false`).

### `newlineKind`

**Default:** `auto`

The kind of newline to use.

* `"auto"` - For each file, uses the newline kind found at the end of the last line.
* `"crlf"` - Uses carriage return, line feed.
* `"lf"` - Uses line feed.
* `"system"` - Uses the system standard (ex. crlf on Windows).

### `useBraces`

**Default:** `whenNotSingleLine`

If braces should be used or not.

* `"whenNotSingleLine"` - Uses braces when the body is on a different line.
* `"maintain"` - Uses braces if they're used. Doesn't use braces if they're not used.
* `"always"` - Forces the use of braces. Will add them if they aren't used.
* `"preferNone"` - Forces no braces when when the header is one line and body is one line. Otherwise forces braces.

[Playground](https://dprint.dev/playground/#code/JYMwBAFALgTgrgUwJRgN5gMYHsB2BnLAGwQDpCsBzCAViQG4wBfAWACg3RJZEk2x-MuAsTKUa9Nh3DR4yNJNYt2rEFhiRiUMMDABeMAAYGOgDxhqxgNSXerAYPxFS5KsAnLV6iNnxatWcCgEPChbex9hZzFQugVPSAi-bRwwIJCUVD4BCKdRKhi2JTYAdwALYGJIEABDQjxkLP4ckRdxWOUyioQq2vqUZqiqWjogA/config/N4IgNglgdgpg6hAJgFwBYgFwA4AMAaEaRGKZBFdDAFgIFcBnGAFQEMAjezAMxbEYMYBbCAGEA9mDFROGZACdaMAdADmYGAEVaY5DBk8+SkPJYRIUFeMGCWMkLABuMOSDqMAQnJYBjPZhAA7qgkAHI6AMqq6gAy0DCuIGxevgAKYvQQyBBS-rAAHsixsACSXAASLBaqCRkW6u5iiACeaRlZORggNtDIplAJ+cjipHISAGKSAa2Z2f2dg0XxBFxicr4AsrRgWYsAgnIqtIIkyPq8-CArazCb2xCLKSxex7pyZ4YEJEcAIjDeYE8WO0oAA6Y6CNjOcIABx81U63VIfQST1GATGtCg3mBAFE8tC5HoMlIQQwYI9CaRgow7IjetAQABfIA)

### `bracePosition`

**Default:** `nextLineIfHanging`

Where to place the opening brace.

* `"maintain"` - Maintains the brace being on the next line or the same line.
* `"sameLine"` - Forces the brace to be on the same line.
* `"nextLine"` - Forces the brace to be on the next line.
* `"nextLineIfHanging"` - Forces the brace to be on the next line if the same line is hanging, but otherwise uses the next.

[Playground](https://dprint.dev/playground/#code/MYGwhgzhAECyCeBhcVoFMAeAXNA7AJjAMoD2AtmgPJYAWaATspBADwRb0CWuA5gDTRcAVzIAjBgOFiGAPmicyABxBoKuLDAQBJdQwBmYYGmgBvALAAoAL6XL3HPQNG48HQ6fHz12xYgB3TixgGmgACghyNFpuHgBKU0sbCzs9MJx2GOgAMizoEloGbNz8unoi6HSscpLCnIq0dmqCsrrKptL4rysgA/config/N4IgNglgdgpg6hAJgFwBYgFwA4AMAaEaRGKZBFdDAFgIFcBnGAFQEMAjezAMxbEYMYBbCAGEA9mDFROGZACdaMAdADmYGAEVaY5DBk8+SkPJYRIUFeMGCWMkLABuMOSDqMAQnJYBjPZhAA7qgkAHI6AMqq6gAy0DCuIGxevgAKYvQQyBBS-rAAHsixsACSXAASLBaqCRkW6u5iiACeaRlZORggNtDIplAJ+cjipHISAGKSAa2Z2f2dg0XxBFxicr4AsrRgWYsAgnIqtIIkyPq8-CArazCb2xCLKSxex7pyZ4YEJEcAIjDeYE8WO0oAA6Y6CNjOcIABx81U63VIfQST1GATGtCg3mBAFE8tC5HoMlIQQwYI9CaRgow7IjetAQABfIA)

### `singleBodyPosition`

**Default:** `maintain`

Where to place the expression of a statement that could possible be on one line (ex. `if (true) console.log(5);`).

* `"maintain"` - Maintains the position of the expression.
* `"sameLine"` - Forces the whole statement to be on one line.
* `"nextLine"` - Forces the expression to be on the next line.

[Playground](https://dprint.dev/playground/#code/JYMwBAFALgTgrgUwJRgN5gMYHsB2BnLAGwQDpCsBzCAViQG4wBfAWACg3RJZEk2x-MuAsTKUa9Nh3AQCAWwRQAFsBwUU2fEVLkqtNgkJ4EYThCxKEMdUK2iqAFl6sDRwZpE6IAZgntWILBhIYigTMABeMAAGBmAwAB4waliAahSnATdhbTFgXzYAoIgNPFDQrHAoBFKMgRLbTyh8-0DIErKTHDAqmrQ+OpsPMSa6NhY-AHdlYkgQAENDZH7+eqHdZqngGYh5xet3HPW6IA/config/N4IgNglgdgpg6hAJgFwBYgFwA4AMAaEaRGKZBFdDAFgIFcBnGAFQEMAjezAMxbEYMYBbCAGEA9mDFROGZACdaMAdADmYGAEVaY5DBk8+SkPJYRIUFeMGCWMkLABuMOSDqMAQnJYBjPZhAA7qgkAHI6AMqq6gAy0DCuIGxevgAKYvQQyBBS-rAAHsixsACSXAASLBaqCRkW6u5iiACeaRlZORggNtDIplAJ+cjipHISAGKSAa2Z2f2dg0XxBFxicr4AsrRgWYsAgnIqtIIkyPq8-CArazCb2xCLKSxex7pyZ4YEJEcAIjDeYE8WO0oAA6Y6CNjOcIABx81U63VIfQST1GATGtCg3mBAFE8tC5HoMlIQQwYI9CaRgow7IjetAQABfIA)

### `nextControlFlowPosition`

**Default:** `nextLine`

Where to place the next control flow within a control flow statement.

* `"maintain"` - Maintains the next control flow being on the next line or the same line.
* `"sameLine"` - Forces the next control flow to be on the same line.
* `"nextLine"` - Forces the next control flow to be on the next line.

[Playground](https://dprint.dev/playground/#code/JYMwBAFALgTgrgUwJRgN4FgBQYdgMYD2AdgM4EA2CAdOQQOYQCsSA3FgL4LkkJiiQEoACwQwUGTIVIVqtBgBZWHLFx5osufMTKUa9CAGYlmdliywAnuuy4pO2fvlG2J-AEMoeIZFFjrdmSpfAhgIX2N2IA/config/N4IgNglgdgpg6hAJgFwBYgFwA4AMAaEaRGKZBFdDAFgIFcBnGAFQEMAjezAMxbEYMYBbCAGEA9mDFROGZACdaMAdADmYGAEVaY5DBk8+SkPJYRIUFeMGCWMkLABuMOSDqMAQnJYBjPZhAA7qgkAHI6AMqq6gAy0DCuIGxevgAKYvQQyBBS-rAAHsixsACSXAASLBaqCRkW6u5iiACeaRlZORggNtDIplAJ+cjipHISAGKSAa2Z2f2dg0XxBFxicr4AsrRgWYsAgnIqtIIkyPq8-CArazCb2xCLKSxex7pyZ4YEJEcAIjDeYE8WO0oAA6Y6CNjOcIABx81U63VIfQST1GATGtCg3mBAFE8tC5HoMlIQQwYI9CaRgow7IjetAQABfIA)

### `trailingCommas`

**Default:** `never`

If trailing commas should be used.

* `"never"` - Trailing commas should not be used.
* `"always"` - Trailing commas should always be used.
* `"onlyMultiLine"` - Trailing commas should only be used in multi-line scenarios.

[Playground](https://dprint.dev/playground/#code/MYewdgzgLgBLC8MDaBYAUDTMCsAadWMALPmgLoDc66oksSArrjAG5kyIgBGAVlWuigBPAA4BTGABUx0DsjAMAtlzEAnZtFUBLMAHNK1NLVncecgN4wQUABZrmUGbAC+-Y7FMAmCwSwQQimKkhI7QpK7oQA/config/N4IgNglgdgpg6hAJgFwBYgFwA4AMAaEaRGKZBFdDAFgIFcBnGAFQEMAjezAMxbEYMYBbCAGEA9mDFROGZACdaMAdADmYGAEVaY5DBk8+SkPJYRIUFeMGCWMkLABuMOSDqMAQnJYBjPZhAA7qgkAHI6AMqq6gAy0DCuIGxevgAKYvQQyBBS-rAAHsixsACSXAASLBaqCRkW6u5iiACeaRlZORggNtDIplAJ+cjipHISAGKSAa2Z2f2dg0XxBFxicr4AsrRgWYsAgnIqtIIkyPq8-CArazCb2xCLKSxex7pyZ4YEJEcAIjDeYE8WO0oAA6Y6CNjOcIABx81U63VIfQST1GATGtCg3mBAFE8tC5HoMlIQQwYI9CaRgow7IjetAQABfIA)

### `forceMultiLineArguments`

**Type:** `boolean`
**Default:** `false`

Forces an argument list to be multi-line when it exceeds the print width.

When false, it will be hanging when the first argument is on the same line as the open parenthesis and multi-line when on a different line.

[Playground](https://dprint.dev/playground/#code/MYewdgzgLgBCBGArGBeGwCGAbLBRAHgA4BOAFFAKbQCWYA5gCoAW1EA8gK5QA0MA7tShMAyiAC2FAEoVsWAJ4AZcIwr4eMStF6b1O7VSgBKANwBYAFAXQkWAkQAmVOlkESpCzE8aD3D15BCFMS+5l5wXBYmFlbg0DBicgCSNhhgwBROYBR8MADCWBgQEOQGtIws7Fy8AkJKUOwAZgCCxHQQvA0gxIlQDCAA4iBsAG5BzBS5TBjECtRiglHm1nEJydCp6Y5oWTn5hcV+njplzKyc6jVMdY0tbR1dPX2DI2NME1MzcwvGQA/config/N4IgNglgdgpg6hAJgFwBYgFwA4AMAaEaRGKZBFdDAFgIFcBnGAFQEMAjezAMxbEYMYBbCAGEA9mDFROGZACdaMAdADmYGAEVaY5DBk8+SkPJYRIUFeMGCWMkLABuMOSDqMAQnJYBjPZhAA7qgkAHI6AMqq6gAy0DCuIGxevgAKYvQQyBBS-rAAHsixsACSXAASLBaqCRkW6u5iiACeaRlZORggNtDIplAJ+cjipHISAGKSAa2Z2f2dg0XxBFxicr4AsrRgWYsAgnIqtIIkyDLyisurG1s7cSksXse6cvq8-CAkRwAiMN5gDyx2lAAHTHQRsZzhAAOPmqnW6pD6CQeowCY1oUG8QIAonkoXI9BkpMCGDB7gTSMFGHYEb1oCAAL5AA)

### `forceMultiLineParameters`

**Type:** `boolean`
**Default:** `false`

Forces a parameter list to be multi-line when it exceeds the print width.

When false, it will be hanging when the first parameter is on the same line as the open parenthesis and multi-line when on a different line.

[Playground](https://dprint.dev/playground/#code/GYVwdgxgLglg9mABFApgZ1mA5gClRmbAFQAsY0B5EKAGkQHcYoSAlFAQwBtOBPAGQRYACuwBO7ALZo6zdlADicCgDcUo0ij4wJTGeigBKRAG8AsACgAvhYuhIsBMn04LiN04LYZZStSNmrIA/config/N4IgNglgdgpg6hAJgFwBYgFwA4AMAaEaRGKZBFdDAFgIFcBnGAFQEMAjezAMxbEYMYBbCAGEA9mDFROGZACdaMAdADmYGAEVaY5DBk8+SkPJYRIUFeMGCWMkLABuMOSDqMAQnJYBjPZhAA7qgkAHI6AMqq6gAy0DCuIGxevgAKYvQQyBBS-rAAHsixsACSXAASLBaqCRkW6u5iiACeaRlZORggNtDIplAJ+cjipHISAGKSAa2Z2f2dg0XxBFxicr4AsrRgWYsAgnIqtIIkyPq8-CArazCb2xCLKSxex7pyMvKKBCRHACIw3mAnix2lAAHTHQRsZzhAAOPmqnW6pD6CSeowCY1oUG8IIAonkYXI9BkpKCGDBHkTSMFGHYkb1oCAAL5A)

### `"arrowFunctionExpression.useParentheses"`

**Default:** `maintain`

Whether to use parentheses around a single parameter in an arrow function.

* `"force"` - Forces parentheses.
* `"maintain"` - Maintains the current state of the parentheses.
* `"preferNone"` - Prefers not using parentheses when possible.

[Playground](https://dprint.dev/playground/#code/MYewdgzgLgBCBGArAjDAvDAFAQwJToD4YBvAXwG4BYAKFElgUQCZ0ZtCSKa7o4kBmVjgBcMaACcAlmADm+NETJVa4XowAsQ7ABoY8eYq7UgA/config/N4IgNglgdgpg6hAJgFwBYgFwA4AMAaEaRGKZBFdDAFgIFcBnGAFQEMAjezAMxbEYMYBbCAGEA9mDFROGZACdaMAdADmYGAEVaY5DBk8+SkPJYRIUFeMGCWMkLABuMOSDqMAQnJYBjPZhAA7qgkAHI6AMqq6gAy0DCuIGxevgAKYvQQyBBS-rAAHsixsACSXAASLBaqCRkW6u5iiACeaRlZORggNtDIplAJ+cjipHISAGKSAa2Z2f2dg0XxBFxicr4AsrRgWYsAgnIqtIIkyPq8-CArazCb2xCLKSxex7pyZ4YEJEcAIjDeYE8WO0oAA6Y6CNjOcIABx81U63VIfQST1GATGtCg3mBAFE8tC5HoMlIQQwYI9CaRgow7IjetAQABfIA)

### `"enumDeclaration.memberSpacing"`

**Default:** `maintain`

How to space the members of an enum.

* `"newline"` - Forces a new line between members.
* `"blankline"` - Forces a blank line between members.
* `"maintain"` - Maintains whether a newline or blankline is used.

[Playground](https://dprint.dev/playground/#code/KYOwrgtgBAKsDOAXKBvAsAKCtqmdQmAgCNgAnARigF4oKAaPHQk8gJhqjcaxwHoAVANgJEASxABzKAL5NsAe0QALcgFkipMpwDMAFh7yoiUROm09mKxgC+mIA/config/N4IgNglgdgpg6hAJgFwBYgFwA4AMAaEaRGKZBFdDAFgIFcBnGAFQEMAjezAMxbEYMYBbCAGEA9mDFROGZACdaMAdADmYGAEVaY5DBk8+SkPJYRIUFeMGCWMkLABuMOSDqMAQnJYBjPZhAA7qgkAHI6AMqq6gAy0DCuIGxevgAKYvQQyBBS-rAAHsixsACSXAASLBaqCRkW6u5iiACeaRlZORggNtDIplAJ+cjipHISAGKSAa2Z2f2dg0XxBFxicr4AsrRgWYsAgnIqtIIkyPq8-CArazCb2xCLKSxex7pyZ4YEJEcAIjDeYE8WO0oAA6Y6CNjOcIABx81U63VIfQST1GATGtCg3mBAFE8tC5HoMlIQQwYI9CaRgow7IjetAQABfIA)

## JSONC

*dprint* has support for JSONC (JSON with comments) via the *dprint-plugin-jsonc* plugin.

Install:

```
yarn add --dev dprint dprint-plugin-typescript dprint-plugin-jsonc
# or
npm install --save-dev dprint dprint-plugin-typescript dprint-plugin-jsonc
```

Add it to *dprint.config.js*:

```js
// @ts-check
const { JsoncPlugin } = require("dprint-plugin-jsonc");

/** @type { import("dprint").Configuration } */
module.exports.config = {
    projectType: "openSource",
    plugins: [
        new JsoncPlugin({})
    ]
};
```

### JSONC - Configuration

There is currently no JSONC specific configuration beyond the global configuration (ex. `lineWidth`, `indentWidth`, etc.).
