// Verifies that the version in the Rust code matches the version in the lock file.
//
// Originally I tried doing this in crates/dprint/build.rs and reading
// the Cargo.lock file, but that doesn't work for publishing because the
// lockfile is outside the crates/cli directory. So the workaround is to
// verify this on the CI instead.
import $ from "https://deno.land/x/dax@0.28.0/mod.ts";

$.logStep("Verifying wasmer-compiler version...");

const rootDir = $.path(import.meta).join("../../../../");
const lockFile = rootDir.join("Cargo.lock");
const rsFile = rootDir.join("crates/dprint/src/plugins/implementations/wasm/setup_wasm_plugin.rs");

const lockText = lockFile.textSync();
const version = lockText.match(/name = "wasmer-compiler"\r?\nversion = "([^"]+)"/m)![1];

$.log(`Found ${version}`);

const rsRegex = /pub const WASMER_COMPILER_VERSION: &str = "([^"]+)"/;
const versionInRs = rsFile.textSync().match(rsRegex)![1];
$.log(`Existing version is: ${versionInRs}`);

if (version === versionInRs) {
  $.log("Version in Rust file is up to date.");
} else {
  $.log("Failed Version in Rust file did not match version in lock file.");
  Deno.exit(1);
}
