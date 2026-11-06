import { deepStrictEqual, rejects } from "node:assert/strict";
import { readJsonWithSizeLimit, ResponseTooLargeError } from "./read_json.ts";

Deno.test("reads json under the limit", async () => {
  deepStrictEqual(await readJsonWithSizeLimit(new Response(`{"a":1}`), 7), { a: 1 });
});

Deno.test("throws when the content length is over the limit", async () => {
  const response = new Response(`{"a":1}`, { headers: { "content-length": "100" } });
  await rejects(() => readJsonWithSizeLimit(response, 50), ResponseTooLargeError);
});

Deno.test("throws when the body is over the limit without a content length", async () => {
  // a stream has no content length, so the limit is enforced while reading
  const body = new ReadableStream<Uint8Array>({
    start(controller) {
      controller.enqueue(new TextEncoder().encode(`{"a":"`));
      controller.enqueue(new TextEncoder().encode("x".repeat(100)));
      controller.enqueue(new TextEncoder().encode(`"}`));
      controller.close();
    },
  });
  await rejects(() => readJsonWithSizeLimit(new Response(body), 50), ResponseTooLargeError);
});
