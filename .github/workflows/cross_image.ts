// Shared config for the loongarch64 cross images that bundle a loongarch64
// build of LLVM (needed by wasmer's LLVM backend). The images are built and
// pushed by cross-loongarch64-images.ts and pulled by ci.ts so that normal CI
// runs don't rebuild LLVM from source.

/** ghcr.io namespace the cross images are published under. */
export const crossImageRegistry = "ghcr.io/dprint";

/** Targets that need a prebuilt cross image (one per cross-rs base). */
export const crossImageTargets = [
  "loongarch64-unknown-linux-gnu",
  "loongarch64-unknown-linux-musl",
];

/**
 * Image tag derived from the Dockerfile contents. Editing the Dockerfile
 * produces a new tag, so the image-build workflow republishes and ci.ts pulls
 * the matching image in lockstep (with a local build fallback for the PR that
 * introduces the change, before the new tag exists).
 */
export const crossImageTag = await computeTag();

async function computeTag() {
  const url = new URL("./cross-loongarch64.Dockerfile", import.meta.url);
  const text = await Deno.readTextFile(url);
  // normalize line endings so the hash is stable across platforms
  const normalized = text.replaceAll("\r\n", "\n");
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(normalized));
  return [...new Uint8Array(digest)].map(b => b.toString(16).padStart(2, "0")).join("").slice(0, 12);
}
