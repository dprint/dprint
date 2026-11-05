/** Thrown when a response's body is larger than allowed. */
export class ResponseTooLargeError extends Error {}

/**
 * Reads a response's body as JSON without reading more than the maximum
 * number of bytes, which ensures a plugin's files can't use up the worker's
 * memory or time.
 */
export async function readJsonWithSizeLimit(response: Response, maxBytes: number): Promise<unknown> {
  // an exact check before reading, which the read also enforces when the
  // header is missing or wrong
  const contentLength = Number(response.headers.get("content-length"));
  if (contentLength > maxBytes) {
    await response.body?.cancel();
    throw new ResponseTooLargeError(`${response.url} is ${contentLength} bytes, which is over ${maxBytes} bytes.`);
  }
  if (response.body == null) {
    throw new Error(`${response.url} doesn't have a body.`);
  }

  const reader = response.body.getReader();
  const chunks: Uint8Array[] = [];
  let size = 0;
  while (true) {
    const { done, value } = await reader.read();
    if (done) {
      break;
    }
    size += value.byteLength;
    if (size > maxBytes) {
      await reader.cancel();
      throw new ResponseTooLargeError(`${response.url} is over ${maxBytes} bytes.`);
    }
    chunks.push(value);
  }

  const bytes = new Uint8Array(size);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return JSON.parse(new TextDecoder().decode(bytes));
}
