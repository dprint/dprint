## TypeScript Code Formatter

Supports:

* TypeScript
* JavaScript - Supports all the JS syntax that the TS compiler supports.
* JSX/TSX

### Install and Setup

Install plugin via:

```bash
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
        new TypeScriptPlugin({
            // Specify TypeScript config here. For example...
            semiColons: false,
        }),
    ],
};
```

Links:

* [Type Declarations](https://github.com/dprint/dprint/blob/master/packages/dprint-plugin-typescript/lib/dprint-plugin-typescript.d.ts)

### `semiColons`

Whether to use semi-colons.

* `"always"` - Always uses semi-colons where applicable.
* `"prefer"` - Prefers to use semi-colons, but doesn't add one in certain scenarios such as for the last member of a single-line type literal (default).
* `"asi"` - Uses automatic semi-colon insertion. Only adds a semi-colon at the start of some expression statements when necessary. Read more: https://standardjs.com/rules.html#semicolons

### `quoteStyle`

How to decide to use single or double quotes.

* `"alwaysDouble"` - Always use double quotes.
* `"alwaysSingle"` - Always use single quotes.
* `"preferDouble"` - Prefer using double quotes except in scenarios where the string contains more double quotes than single quotes (default).
* `"preferSingle"` - Prefer using single quotes except in scenarios where the string contains more single quotes than double quotes.

### `newLineKind`

The kind of newline to use.

* `"auto"` - For each file, uses the newline kind found at the end of the last line.
* `"crlf"` - Uses carriage return, line feed.
* `"lf"` - Uses line feed (default).
* `"system"` - Uses the system standard (ex. crlf on Windows).

### `useBraces`

If braces should be used or not.

* `"whenNotSingleLine"` - Uses braces when the body is on a different line (default).
* `"maintain"` - Uses braces if they're used. Doesn't use braces if they're not used.
* `"always"` - Forces the use of braces. Will add them if they aren't used.
* `"preferNone"` - Forces no braces when when the header is one line and body is one line. Otherwise forces braces.

[Playground](https://dprint.dev/playground/#code/JYMwBAFALgTgrgUwJRgN5gMYHsB2BnLAGwQDpCsBzCAViQG4wBfAWACg3RJZEk2x-MuAsTKUa9Nh3DR4yNJNYt2rEFhiRiUMMDABeMAAYGOgDxhqxgNSXerAYPxFS5KsAnLV6iNnxatWcCgEPChbex9hZzFQugVPSAi-bRwwIJCUVD4BCKdRKhi2JTYAdwALYGJIEABDQjxkLP4ckRdxWOUyioQq2vqUZqiqWjogA/config/N4IgNglgdgpg6hAJgFwBYgFwA4AMAaEaRGKZBFdDAFgIFcBnGAFQEMAjezAMxbEYMYBbCAGEA9mDFROGZACdaMAdADmYGAEVaY5DBk8+SkPJYRIUFeMGCWMkLABuMOSDqMAQnJYBjPZhAA7qgkAHI6AMqq6gAy0DCuIGxevgAKYvQQyBBS-rAAHsixsACSXAASLBaqCRkW6u5iiACeaRlZORggNtDIplAJ+cjipHISAGKSAa2Z2f2dg0XxBFxicr4AsrRgWYsAgnIqtIIkyPq8-CArazCb2xCLKSxex7pyZ4YEJEcAIjDeYE8WO0oAA6Y6CNjOcIABx81U63VIfQST1GATGtCg3mBAFE8tC5HoMlIQQwYI9CaRgow7IjetAQABfIA)

### `bracePosition`

Where to place the opening brace.

* `"maintain"` - Maintains the brace being on the next line or the same line.
* `"sameLine"` - Forces the brace to be on the same line.
* `"nextLine"` - Forces the brace to be on the next line.
* `"nextLineIfHanging"` - Forces the brace to be on the next line if the same line is hanging, but otherwise uses the next (default).

[Playground](https://dprint.dev/playground/#code/MYGwhgzhAECyCeBhcVoFMAeAXNA7AJjAMoD2AtmgPJYAWaATspBADwRb0CWuA5gDTRcAVzIAjBgOFiGAPmicyABxBoKuLDAQBJdQwBmYYGmgBvALAAoAL6XL3HPQNG48HQ6fHz12xYgB3TixgGmgACghyNFpuHgBKU0sbCzs9MJx2GOgAMizoEloGbNz8unoi6HSscpLCnIq0dmqCsrrKptL4rysgA/config/N4IgNglgdgpg6hAJgFwBYgFwA4AMAaEaRGKZBFdDAFgIFcBnGAFQEMAjezAMxbEYMYBbCAGEA9mDFROGZACdaMAdADmYGAEVaY5DBk8+SkPJYRIUFeMGCWMkLABuMOSDqMAQnJYBjPZhAA7qgkAHI6AMqq6gAy0DCuIGxevgAKYvQQyBBS-rAAHsixsACSXAASLBaqCRkW6u5iiACeaRlZORggNtDIplAJ+cjipHISAGKSAa2Z2f2dg0XxBFxicr4AsrRgWYsAgnIqtIIkyPq8-CArazCb2xCLKSxex7pyZ4YEJEcAIjDeYE8WO0oAA6Y6CNjOcIABx81U63VIfQST1GATGtCg3mBAFE8tC5HoMlIQQwYI9CaRgow7IjetAQABfIA)

### `singleBodyPosition`

Where to place the expression of a statement that could possible be on one line (ex. `if (true) console.log(5);`).

* `"maintain"` - Maintains the position of the expression (default).
* `"sameLine"` - Forces the whole statement to be on one line.
* `"nextLine"` - Forces the expression to be on the next line.

[Playground](https://dprint.dev/playground/#code/JYMwBAFALgTgrgUwJRgN5gMYHsB2BnLAGwQDpCsBzCAViQG4wBfAWACg3RJZEk2x-MuAsTKUa9Nh3AQCAWwRQAFsBwUU2fEVLkqtNgkJ4EYThCxKEMdUK2iqAFl6sDRwZpE6IAZgntWILBhIYigTMABeMAAGBmAwAB4waliAahSnATdhbTFgXzYAoIgNPFDQrHAoBFKMgRLbTyh8-0DIErKTHDAqmrQ+OpsPMSa6NhY-AHdlYkgQAENDZH7+eqHdZqngGYh5xet3HPW6IA/config/N4IgNglgdgpg6hAJgFwBYgFwA4AMAaEaRGKZBFdDAFgIFcBnGAFQEMAjezAMxbEYMYBbCAGEA9mDFROGZACdaMAdADmYGAEVaY5DBk8+SkPJYRIUFeMGCWMkLABuMOSDqMAQnJYBjPZhAA7qgkAHI6AMqq6gAy0DCuIGxevgAKYvQQyBBS-rAAHsixsACSXAASLBaqCRkW6u5iiACeaRlZORggNtDIplAJ+cjipHISAGKSAa2Z2f2dg0XxBFxicr4AsrRgWYsAgnIqtIIkyPq8-CArazCb2xCLKSxex7pyZ4YEJEcAIjDeYE8WO0oAA6Y6CNjOcIABx81U63VIfQST1GATGtCg3mBAFE8tC5HoMlIQQwYI9CaRgow7IjetAQABfIA)

### `nextControlFlowPosition`

Where to place the next control flow within a control flow statement.

* `"maintain"` - Maintains the next control flow being on the next line or the same line.
* `"sameLine"` - Forces the next control flow to be on the same line.
* `"nextLine"` - Forces the next control flow to be on the next line (default).

[Playground](https://dprint.dev/playground/#code/JYMwBAFALgTgrgUwJRgN4FgBQYdgMYD2AdgM4EA2CAdOQQOYQCsSA3FgL4LkkJiiQEoACwQwUGTIVIVqtBgBZWHLFx5osufMTKUa9CAGYlmdliywAnuuy4pO2fvlG2J-AEMoeIZFFjrdmSpfAhgIX2N2IA/config/N4IgNglgdgpg6hAJgFwBYgFwA4AMAaEaRGKZBFdDAFgIFcBnGAFQEMAjezAMxbEYMYBbCAGEA9mDFROGZACdaMAdADmYGAEVaY5DBk8+SkPJYRIUFeMGCWMkLABuMOSDqMAQnJYBjPZhAA7qgkAHI6AMqq6gAy0DCuIGxevgAKYvQQyBBS-rAAHsixsACSXAASLBaqCRkW6u5iiACeaRlZORggNtDIplAJ+cjipHISAGKSAa2Z2f2dg0XxBFxicr4AsrRgWYsAgnIqtIIkyPq8-CArazCb2xCLKSxex7pyZ4YEJEcAIjDeYE8WO0oAA6Y6CNjOcIABx81U63VIfQST1GATGtCg3mBAFE8tC5HoMlIQQwYI9CaRgow7IjetAQABfIA)

### `operatorPosition`

Where to place the operator for expressions that span multiple lines.

* `"maintain"` - Maintains the operator being on the next line or the same line.
* `"sameLine"` - Forces the operator to be on the same line.
* `"nextLine"` - Forces the operator to be on the next line (default)

[Playground](https://dprint.dev/playground/#code/MYewdgzgLgBATgUwgVwDawLwwJYQCpzIIwBkJMYIUAogI7ICGqeIAsAFAxenkQgC2CAMLgAJtijZwMAD4yO3GFQAWCOCLDjJ4ANxA/config/N4IgNglgdgpg6hAJgFwBYgFwA4AMAaEaRGKZBFdDAFgIFcBnGAFQEMAjezAMxbEYMYBbCAGEA9mDFROGZACdaMAdADmYGAEVaY5DBk8+SkPJYRIUFeMGCWMkLABuMOSDqMAQnJYBjPZhAA7qgkAHI6AMqq6gAy0DCuIGxevgAKYvQQyBBS-rAAHsixsACSXAASLBaqCRkW6u5iiACeaRlZORggNtDIplAJ+cjipHISAGKSAa2Z2f2dg0XxBGIADs4syGJy0+1z9jAFiwlcW74AsrRgWYsAgnIqtIIkyPq8-CAncueX13EpLF4nro5K9DAQSI8ACIwbxgAEbWYAOiegjYznCKx81U63VIfQSANGATGtCg3l2AFE8is5HoMlJEQwYP9aaRgow7LjetAQABfIA)

### `trailingCommas`

If trailing commas should be used.

* `"never"` - Trailing commas should not be used.
* `"always"` - Trailing commas should always be used.
* `"onlyMultiLine"` - Trailing commas should only be used in multi-line scenarios (default).

[Playground](https://dprint.dev/playground/#code/MYewdgzgLgBLC8MDaBYAUDTMCsAadWMALPmgLoDc66oksSArrjAG5kyIgBGAVlWuigBPAA4BTGABUx0DsjAMAtlzEAnZtFUBLMAHNK1NLVncecgN4wQUABZrmUGbAC+-Y7FMAmCwSwQQimKkhI7QpK7oQA/config/N4IgNglgdgpg6hAJgFwBYgFwA4AMAaEaRGKZBFdDAFgIFcBnGAFQEMAjezAMxbEYMYBbCAGEA9mDFROGZACdaMAdADmYGAEVaY5DBk8+SkPJYRIUFeMGCWMkLABuMOSDqMAQnJYBjPZhAA7qgkAHI6AMqq6gAy0DCuIGxevgAKYvQQyBBS-rAAHsixsACSXAASLBaqCRkW6u5iiACeaRlZORggNtDIplAJ+cjipHISAGKSAa2Z2f2dg0XxBFxicr4AsrRgWYsAgnIqtIIkyPq8-CArazCb2xCLKSxex7pyZ4YEJEcAIjDeYE8WO0oAA6Y6CNjOcIABx81U63VIfQST1GATGtCg3mBAFE8tC5HoMlIQQwYI9CaRgow7IjetAQABfIA)

### `preferHanging`

When `true` (non-default), Dprint will prefer hanging indentation instead of making code split up on multiple lines.

[Playground](https://dprint.dev/playground/#code/MYewdgzgLgBApgDwIYFsAOAbOMC8MDeMEIKcAanAE4CeAMuAOYBcMArKwMwAsAHKwJwB2bgBoYIAEYArOMCi0AllCpIMAUQRpKcCBAXgWrLhyPdWANhgBfALAAoe6EixipAIKVKSarhgBtY04ufgAGVh5+DhD+LnDuELEAJjNoiLCeWOjRGA4Y9NyQ2J5crnzYgF17R1V1TUoACmVoBTAGABUACwUIAHkAVygxAHclDoBlEjgAJTgaukY2xEGYJuXVsXWVnWWQKA6qRegASgBuaoxarXr7GFut6BEbu939yke7O-EB+1Pzy4bWGJzGJBL8HHYnNAYChqABJZxIMDAbB4MBwIYwADCGCQuka2xa7S6vQGw1G9CgvQAZh4GBAxFSQJRYVA2iAAOIgHoANwO+0xHSQlEUKCUYMhsBh8OgiORiV8aIx2NxEGuHzuq0JnW6-WWIz2FOptPpMEZzNZHK5vMonTgAqFIrFZwh4ChUoRSLgHAV6KxOLxmta2pJetGE3clDpACE4BgQENHVAwfYqX0kVB9GB7lAAIz45pB4m6sl7GZzeitAAKQtQJr2SCgnJ5fLgiY2BNaRwI9ls4NT6cz2cSas+gYYGyLAy7+B7VTs-bkg9WHHzGdaE51U+7disQA/config/N4IgNglgdgpg6hAJgFwBYgFwA4AMAaEaRGKZBFdDAFgIFcBnGAFQEMAjezAMxbEYMYBbCAGEA9mDFROGZACdaMAdADmYGAEVaY5DBk8+SkPJYRIUFeMGCWMkLABuMOSDqMAQnJYBjPZhAA7qgkAHI6AMqq6gAy0DCuIGxevgAKYvQQyBBS-rAAHsixsACSXAASLBaqCRkW6u5iiACeaRlZORggNtDIplAJ+cjipHISAGKSAa2Z2f2d9CyCMEXxBGIADs4syGJy0+1z9jAFKwnrcjBczhVVFpjyigQktIIAIjDeYCxeBwB0S4I2M5wusfNVOt1SH0Et9RgExrQoN4DgBRPLnPQZKS-BgwFLfEhoPR+CF9XrQEAAXyAA)

### `preferSingleLine`

**Note:** This configuration option was recently added and is still undergoing lots of changes.

When `false` (default), certain code will be allowed to span multiple lines even when it could possibly fit on a single line.

For example, if the first parameter or argument is placed on a different line than the open parenthesis then the entire argument or parameter list will become multi-line.

```ts
callExpr(1, 2, 3);
// formats as
callExpr(1, 2, 3);

// but...
callExpr(
    1, 2, 3);
// formats as
callExpr(
    1,
    2,
    3,
);
```

To switch back to a single line, place the first argument or parameter on the same line as the open parenthesis:

```ts
callExpr(1,
    2,
    3,
);
// formats as
callExpr(1, 2, 3);
```

However, when `preferSingleLine` is `true`, then...

```ts
callExpr(
    1,
    2,
    3,
);
// formats as
callExpr(1, 2, 3);
```

If you would like to force something to be multi-line when `preferSingleLine` is `true`, then add a comment to the front or beside an item:

```ts
call(
    // force multi-line
    1,
    2,
    3,
);
```

Note: Turning `preferSingleLine` on might cause more code to prefer being on a single line than you are used to. You may want to disable this feature on a per-node basis (ex. `"objectExpression.preferSingleLine": false`).

### Space separators

There are individual configuration options for setting the space settings on a per AST node basis. For example:

```ts
module.exports.config = {
    projectType: "openSource",
    plugins: [
        new TypeScriptPlugin({
            "constructorType.spaceAfterNewKeyword": true,
        }),
    ],
};
```

```ts
// formats...
type CtorOf<T> = new(...args) => T;
// as...
type CtorOf<T> = new (...args) => T;
```

See the `TypeScriptConfiguration` interface in the [type declarations](https://github.com/dprint/dprint/blob/master/packages/dprint-plugin-typescript/lib/dprint-plugin-typescript.d.ts) and search for options that begin with the word "space" to see all the possibilities.

### `"arrowFunction.useParentheses"`

Whether to use parentheses around a single parameter in an arrow function.

* `"force"` - Forces parentheses.
* `"maintain"` - Maintains the current state of the parentheses (default).
* `"preferNone"` - Prefers not using parentheses when possible.

[Playground](https://dprint.dev/playground/#code/MYewdgzgLgBCBGArAjDAvDAFAQwJToD4YBvAXwG4BYAKFElgUQCZ0ZtCSKa7o4kBmVjgBcMaACcAlmADm+NETJVa4XowAsQ7ABoY8eYq7UgA/config/N4IgNglgdgpg6hAJgFwBYgFwA4AMAaEaRGKZBFdDAFgIFcBnGAFQEMAjezAMxbEYMYBbCAGEA9mDFROGZACdaMAdADmYGAEVaY5DBk8+SkPJYRIUFeMGCWMkLABuMOSDqMAQnJYBjPZhAA7qgkAHI6AMqq6gAy0DCuIGxevgAKYvQQyBBS-rAAHsixsACSXAASLBaqCRkW6u5iiACeaRlZORggNtDIplAJ+cjipHISAGKSAa2Z2f2dg0XxBFxicr4AsrRgWYsAgnIqtIIkyPq8-CArazCb2xCLKSxex7pyZ4YEJEcAIjDeYE8WO0oAA6Y6CNjOcIABx81U63VIfQST1GATGtCg3mBAFE8tC5HoMlIQQwYI9CaRgow7IjetAQABfIA)

### `"enumDeclaration​.memberSpacing"`

How to space the members of an enum.

* `"newline"` - Forces a new line between members.
* `"blankline"` - Forces a blank line between members.
* `"maintain"` - Maintains whether a newline or blankline is used (default).

[Playground](https://dprint.dev/playground/#code/KYOwrgtgBAKsDOAXKBvAsAKCtqmdQmAgCNgAnARigF4oKAaPHQk8gJhqjcaxwHoAVANgJEASxABzKAL5NsAe0QALcgFkipMpwDMAFh7yoiUROm09mKxgC+mIA/config/N4IgNglgdgpg6hAJgFwBYgFwA4AMAaEaRGKZBFdDAFgIFcBnGAFQEMAjezAMxbEYMYBbCAGEA9mDFROGZACdaMAdADmYGAEVaY5DBk8+SkPJYRIUFeMGCWMkLABuMOSDqMAQnJYBjPZhAA7qgkAHI6AMqq6gAy0DCuIGxevgAKYvQQyBBS-rAAHsixsACSXAASLBaqCRkW6u5iiACeaRlZORggNtDIplAJ+cjipHISAGKSAa2Z2f2dg0XxBFxicr4AsrRgWYsAgnIqtIIkyPq8-CArazCb2xCLKSxex7pyZ4YEJEcAIjDeYE8WO0oAA6Y6CNjOcIABx81U63VIfQST1GATGtCg3mBAFE8tC5HoMlIQQwYI9CaRgow7IjetAQABfIA)

### AST Node Specific Configuration

In most situations, configuration can be set for specific kinds of declarations, statements, and expressions.

For example, you can specify the general `nextControlFlowPosition`, but you can also specify a more specific `"tryStatement.nextControlFlowPosition"` option that will be used for that statement.

```ts
module.exports.config = {
    projectType: "openSource",
    plugins: [
        new TypeScriptPlugin({
            nextControlFlowPosition: "maintain",
            "tryStatement.nextControlFlowPosition": "sameLine"
        }),
    ],
};
```

### Ignoring Files

Add an ignore file comment as the **first** comment in the file:

```ts
// dprint-ignore-file
```

[Playground](https://dprint.dev/playground/#code/PTAEBMAcCcEsDsAuBaWBzeB7aBTZAzWAGxwFgAoAY03gGdFRZwclZEBPUAXlAG0LQg0AEYANKAAM4qQKFSR00bMHz5wigF0A3BQrU6DJi0Rt2AJm59lCyYutq75IbZfry2oA/config/N4IgNglgdgpg6hAJgFwBYgFwA4AMAaEaRGKZBFdDAFgIFcBnGAFQEMAjezAMxbEYMYBbCAGEA9mDFROGZACdaMAdADmYGAEVaY5DBk8+SkPJYRIUFeMGCWMkLABuMOSDqMAQnJYBjPZhAA7qgkAHI6AMqq6gAy0DCuIGxevgAKYvQQyBBS-rAAHsixsACSXAASLBaqCRkW6u5iiACeaRlZORggNtDIplAJ+cjipHISAGKSAa2Z2f2dg0XxBFxicr4AsrRgWYsAgnIqtIIkyPq8-CArazCb2xCLKSxex7pyZ4YEJEcAIjDeYE8WO0oAA6Y6CNjOcIABx81U63VIfQST1GATGtCg3mBAFE8tC5HoMlIQQwYI9CaRgow7IjetAQABfIA)

### Ignore Comments

Add an ignore comment before the code:

```ts
// dprint-ignore
const identity = [
    1, 0, 0,
    0, 1, 0,
    0, 0, 1,
];

// or even...

const identity = /* dprint-ignore */ [
    1, 0, 0,
    0, 1, 0,
    0, 0, 1,
];
```

[Playground](https://dprint.dev/playground/#code/PTAEBMAcCcEsDsAuBaWBzeB7aBTAsAFADGm8AzoqLODkrIgJ6gC8oA2oaF6AIwA0oAAwDhnbsN4i+YrhIk9CAXQDchQiFDZQOAG60AdIbXFSFblRp1GAJm6tgAKggwEKdFlygHYDgXP8hKRlAyUDguQEFAhUgA/config/N4IgNglgdgpg6hAJgFwBYgFwA4AMAaEaRGKZBFdDAFgIFcBnGAFQEMAjezAMxbEYMYBbCAGEA9mDFROGZACdaMAdADmYGAEVaY5DBk8+SkPJYRIUFeMGCWMkLABuMOSDqMAQnJYBjPZhAA7qgkAHI6AMqq6gAy0DCuIGxevgAKYvQQyBBS-rAAHsixsACSXAASLBaqCRkW6u5iiACeaRlZORggNtDIplAJ+cjipHISAGKSAa2Z2f2dg0XxBFxicr4AsrRgWYsAgnIqtIIkyPq8-CArazCb2xCLKSxex7pyZ4YEJEcAIjDeYE8WO0oAA6Y6CNjOcIABx81U63VIfQST1GATGtCg3mBAFE8tC5HoMlIQQwYI9CaRgow7IjetAQABfIA)

### Explicit Newlines

For the most part, dprint allows you to place certain nodes like binary, logical, and member expressions on different lines as you see fit. It does this because newlines can often convey meaning or grouping.

```ts
// formats this as-is
const mathResult = 1 + 2 * 6
    + moreMath * math;

expect(someFunctionCall(1, 2))
    .to.equal(5);
```

Also, placing a node on the next line after an open paren will indent the text within the parens.

```ts
const mathResult = (
    1 + 2);
// formats as
const mathResult = (
    1 + 2
);
```

The same happens with statements like if statements.

```ts
if (
    someCondition && otherCondition) {
}
// formats as
if (
    someCondition && otherCondition
) {
}
```

[Playground](https://dprint.dev/playground/#code/MYewdgzgLgBAtgQygCwEoFMIFcA2sC8MAjDANQwBMMAVDAGwCwAUDK2fCAE7oCySyNePwDczZugAeAB3TAoACggg46AGJYwcgJbgAwghw55RADSUAlOeZsYAOighb6AI5YD8gKznRTZqEiwiCgY2HhUhPLWbCTkFN5iTFoAZjCRLGxKKrrgACZaUDpgMABkxTAgKOic2WB5BeDmMADezAC+QA/config/N4IgNglgdgpg6hAJgFwBYgFwA4AMAaEaRGKZBFdDAFgIFcBnGAFQEMAjezAMxbEYMYBbCAGEA9mDFROGZACdaMAdADmYGAEVaY5DBk8+SkPJYRIUFeMGCWMkLABuMOSDqMAQnJYBjPZhAA7qgkAHI6AMqq6gAy0DCuIGxevgAKYvQQyBBS-rAAHsixsACSXAASLBaqCRkW6u5iiACeaRlZORggNtDIplAJ+cjipHISAGKSAa2Z2f2dg0XxBFxicr4AsrRgWYsAgnIqtIIkyPq8-CArazCb2xCLKSxex7pyZ4YEJEcAIjDeYE8WO0oAA6Y6CNjOcIABx81U63VIfQST1GATGtCg3mBAFE8tC5HoMlIQQwYI9CaRgow7IjetAQABfIA)

#### Forcing a Line Per Expression

By default, dprint will leave line breaks between expressions in member expressions (ex. `myObj.prop`) and binary expressions (ex. `value + other`). If you don't want this behaviour, you can disable it by setting the following configuration:

* `"memberExpression.linePerExpression": true`
* `"binaryExpression.linePerExpression": true`

Example:

```ts
myObject.accessing.someProperty;
myObject
    .accessing.some
    .other.prop;
myObject.myLooooooooooonnnnnggggggg.propAccess;
// formats as (when line width is 40)
myObject.accessing.someProperty;
myObject
    .accessing
    .some
    .other
    .prop;
myObject
    .myLooooooooooonnnnnggggggg
    .propAccess;
```

You may want to use both `"preferSingleLine": true` in combination with this option:

```ts
myObject.accessing.someProperty;
myObject
    .accessing.some
    .other.prop;
myObject.myLooooooooooonnnnnggggggg.propAccess;
// formats as (when line width is 40)
myObject.accessing.someProperty;
myObject.accessing.some.other.prop;
myObject
    .myLooooooooooonnnnnggggggg
    .propAccess;
```

### Statement & Member Spacing

Line breaks are maintained, but not when they are consecutive or if they are at the beginning or end of a block.

[Playground](https://dprint.dev/playground/#code/GYVwdgxgLglg9mABAWwJ4DFzXmAFASkQG8BYAKHMSuqoHpbEAZGMAU0QCMAnVgQwGsAzogDuMADbjO7ZLxZQ5bACYAaTiCiIwcTSIAWrJFAOpEvHuUtkaNeoggJBrCBpgA3dnC6IYwRMdZTc3ZeTQDpAHMWMBYIxC9EQyV4v15OcTgIfgA6KxtqHigQLiQAVgBuKwBfK3lWLmBeCHYAWVQASTAoesbm4jzqAAcuOEGALkRBKC5Yyop5mh0DLgAFEfGtEGQOernKamRWYzglAgmwLZ2uOcWArhajvROzxDc4GCU9siqgA/config/N4IgNglgdgpg6hAJgFwBYgFwA4AMAaEaRGKZBFdDAFgIFcBnGAFQEMAjezAMxbEYMYBbCAGEA9mDFROGZACdaMAdADmYGAEVaY5DBk8+SkPJYRIUFeMGCWMkLABuMOSDqMAQnJYBjPZhAA7qgkAHI6AMqq6gAy0DCuIGxevgAKYvQQyBBS-rAAHsixsACSXAASLBaqCRkW6u5iiACeaRlZORggNtDIplAJ+cjipHISAGKSAa2Z2f2dg0XxBFxicr4AsrRgWYsAgnIqtIIkyPq8-CArazCb2xCLKSxex7pyZ4YEJEcAIjDeYE8WO0oAA6Y6CNjOcIABx81U63VIfQST1GATGtCg3mBAFE8tC5HoMlIQQwYI9CaRgow7IjetAQABfIA)

## JSONC

*dprint* has support for JSONC (JSON with comments) via the *dprint-plugin-jsonc* plugin.

Install:

```bash
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
        new JsoncPlugin({}),
    ],
};
```
