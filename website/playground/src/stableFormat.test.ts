/// <reference lib="deno.ns" />

import { strictEqual, throws } from "node:assert/strict";
import { formatTextUntilStable } from "./stableFormat.ts";

Deno.test("returns unchanged text after one pass", () => {
  let callCount = 0;
  const result = formatTextUntilStable("text", text => {
    callCount++;
    return text;
  });

  strictEqual(result, "text");
  strictEqual(callCount, 1);
});

Deno.test("formats text until stable", () => {
  const input = `const obj = {
    prop: (value),
    nested: ((other)),
};
`;
  const firstPass = `const obj = {
  prop: value,
  nested: (other),
};
`;
  const stableText = `const obj = {
  prop: value,
  nested: other,
};
`;
  const outputs = new Map([
    [input, firstPass],
    [firstPass, stableText],
    [stableText, stableText],
  ]);
  let callCount = 0;

  const result = formatTextUntilStable(input, text => {
    callCount++;
    const output = outputs.get(text);
    if (output == null) {
      throw new Error("Unexpected input.");
    }
    return output;
  });

  strictEqual(result, stableText);
  strictEqual(callCount, 3);
});

Deno.test("errors when formatting does not stabilize", () => {
  let callCount = 0;

  throws(
    () =>
      formatTextUntilStable("text", text => {
        callCount++;
        return `${text}_formatted`;
      }),
    /Formatting not stable\. Bailed after 5 tries\./,
  );
  strictEqual(callCount, 6);
});
