---
title: TypeScript / JavaScript Plugin
description: Documentation on the TypeScript / JavaScript code formatting plugin for dprint.
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/typescript">TypeScript</a></li>
  </ul>
</nav>

# TypeScript / JavaScript Code Formatter

Supports:

- TypeScript
- JavaScript - Supports all the JS syntax that the TS compiler supports.
- JSX/TSX

## Install and Setup

In _.dprintrc.json_:

1. Specify the plugin url in the `"plugins"` array.
2. Ensure `.ts,.tsx,.js,.jsx` file extensions are matched in an `"includes"` pattern.
3. Add a `"typescript"` configuration property if desired.

```json
{
  // ...etc...
  "typescript": {
    // TypeScript & JavaScript config goes here
  },
  "plugins": [
    // ...etc...
    "https://plugins.dprint.dev/typescript-x.x.x.wasm"
  ]
}
```

## Configuration

See [Configuration](/plugins/typescript/config)

## Playground

See [Playground](https://dprint.dev/playground#language/typescript)

## Ignoring Files

Add an ignore file comment as the **first** comment in the file:

```ts
// dprint-ignore-file
```

## Ignore Comments

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

## Explicit Newlines

For the most part, dprint allows you to place certain nodes like binary, logical, and member expressions on different lines as you see fit. It does this because newlines can often convey meaning or grouping.

```ts
// formats this as-is
const mathResult = 1 + 2 * 6
    + moreMath * math;

expect(someFunctionCall(1, 2))
    .to.equal(5);
```

Also, placing a node on the next line after an open paren will indent the text within the parens.

<!-- dprint-ignore -->
```ts
const mathResult = (
    1 + 2);
// formats as
const mathResult = (
    1 + 2
);
```

The same happens with statements like if statements.

<!-- dprint-ignore -->
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

[Playground](https://dprint.dev/playground/#code/MYewdgzgLgBAtgQygCwEoFMIFcA2sC8MAjDANQwBMMAVDAGwCwAUDK2fCAE7oCySyNePwDczZugAeAB3TAoACggg46AGJYwcgJbgAwghw55RADSUAlOeZsYAOighb6AI5YD8gKznRTZqEiwiCgY2HhUhPLWbCTkFN5iTFoAZjCRLGxKKrrgACZaUDpgMABkxTAgKOic2WB5BeDmMADezAC+QA/config/N4KAviQ/language/typescript)

### Forcing a Line Per Expression

By default, dprint will leave line breaks between expressions in member expressions (ex. `myObj.prop`) and binary expressions (ex. `value + other`). If you don't want this behaviour, you can disable it by setting the following configuration:

- `"memberExpression.linePerExpression": true`
- `"binaryExpression.linePerExpression": true`

Example:

<!-- dprint-ignore -->
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

<!-- dprint-ignore -->
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

## Statement & Member Spacing

Line breaks are maintained, but not when they are consecutive or if they are at the beginning or end of a block.

[Playground](https://dprint.dev/playground/#code/GYVwdgxgLglg9mABAWwJ4DFzXmAFASkQG8BYAKHMSuqoHpbEAZGMAU0QCMAnVgQwGsAzogDuMADbjO7ZLxZQ5bACYAaTiCiIwcTSIAWrJFAOpEvHuUtkaNeoggJBrCBpgA3dnC6IYwRMdZTc3ZeTQDpAHMWMBYIxC9EQyV4v15OcTgIfgA6KxtqHigQLiQAVgBuKwBfK3lWLmBeCHYAWVQASTAoesbm4jzqAAcuOEGALkRBKC5Yyop5mh0DLgAFEfGtEGQOernKamRWYzglAgmwLZ2uOcWArhajvROzxDc4GCU9siqgA/config/N4KAviQ/language/typescript)
