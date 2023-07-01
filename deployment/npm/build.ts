import $ from "https://deno.land/x/dax@0.32.0/mod.ts";
import { decompress } from "https://deno.land/x/zip@v1.2.5/decompress.ts";

interface Package {
  zipFileName: string;
  os: "win32" | "darwin" | "linux";
  cpu: "x64" | "arm64";
  libc?: "glibc" | "musl";
}

const packages: Package[] = [{
  zipFileName: "dprint-x86_64-pc-windows-msvc.zip",
  os: "win32",
  cpu: "x64",
}, {
  zipFileName: "dprint-x86_64-apple-darwin.zip",
  os: "darwin",
  cpu: "x64",
}, {
  zipFileName: "dprint-aarch64-apple-darwin.zip",
  os: "darwin",
  cpu: "arm64",
}, {
  zipFileName: "dprint-x86_64-unknown-linux-gnu.zip",
  os: "linux",
  cpu: "x64",
  libc: "glibc",
}, {
  zipFileName: "dprint-x86_64-unknown-linux-musl.zip",
  os: "linux",
  cpu: "x64",
  libc: "musl",
}, {
  zipFileName: "dprint-aarch64-unknown-linux-gnu.zip",
  os: "linux",
  cpu: "arm64",
  libc: "glibc",
}];

const markdownText = `# dprint

npm CLI distribution for [dprint](https://dprint.dev)—a pluggable and configurable code formatting platform.
`;

const currentDir = $.path(import.meta).parentOrThrow();
const rootDir = currentDir.parentOrThrow().parentOrThrow();
const outputDir = currentDir.join("./dist");
const dprintDir = outputDir.join("dprint");

await $`rm -rf ${outputDir}`;
await $`mkdir -p ${dprintDir}`;

const version = Deno.args[0];

if (version == null) {
  throw new Error("Please provide a version as the first argument.");
}

// setup dprint packages
$.logStep(`Setting up dprint ${version}...`);
const pkgJson = {
  "name": "dprint",
  "version": version,
  "description": "Pluggable and configurable code formatting platform written in Rust.",
  "bin": "bin.js",
  "repository": {
    "type": "git",
    "url": "git+https://github.com/dprint/dprint.git",
  },
  "keywords": [
    "code",
    "formatter",
  ],
  "author": "David Sherret",
  "license": "MIT",
  "bugs": {
    "url": "https://github.com/dprint/dprint/issues",
  },
  "homepage": "https://github.com/dprint/dprint#readme",
  optionalDependencies: packages
    .map(pkg => `@dprint/${getPackageNameNoScope(pkg)}`)
    .reduce((obj, pkgName) => ({ ...obj, [pkgName]: version }), {}),
};
currentDir.join("bin.js").copyFileSync(dprintDir.join("bin.js"));
currentDir.join("install_api.js").copyFileSync(dprintDir.join("install_api.js"));
dprintDir.join("package.json")
  .writeTextSync(JSON.stringify(pkgJson, undefined, 2) + "\n");
rootDir.join("LICENSE").copyFileSync(dprintDir.join("LICENSE"));
dprintDir.join("README.md").writeTextSync(markdownText);

// setup each binary package
for (const pkg of packages) {
  const pkgName = getPackageNameNoScope(pkg);
  $.logStep(`Setting up @dprint/${pkgName}...`);
  const pkgDir = outputDir.join(pkgName);
  const zipPath = pkgDir.join("output.zip");

  await $`mkdir -p ${pkgDir}`;

  // download and extract the zip file
  const zipUrl = `https://github.com/dprint/dprint/releases/download/${version}/${pkg.zipFileName}`;
  await $.request(zipUrl).showProgress().pipeToPath(zipPath);
  await decompress(zipPath.toString(), pkgDir.toString());
  zipPath.removeSync();

  // create the package.json and readme
  pkgDir.join("README.md").writeTextSync(`# @dprint/${pkgName}\n\n${pkgName} distribution of dprint.\n`);
  pkgDir.join("package.json").writeTextSync(
    JSON.stringify(
      {
        "name": `@dprint/${pkgName}`,
        "version": version,
        "description": `${pkgName} distribution of the dprint code formatter`,
        "bin": "bin.js",
        "repository": {
          "type": "git",
          "url": "git+https://github.com/dprint/dprint.git",
        },
        // force yarn to unpack
        "preferUnplugged": true,
        "author": "David Sherret",
        "license": "MIT",
        "bugs": {
          "url": "https://github.com/dprint/dprint/issues",
        },
        "homepage": "https://github.com/dprint/dprint#readme",
        "os": [pkg.os],
        "cpu": [pkg.cpu],
        libc: pkg.libc == null ? undefined : [pkg.libc],
      },
      null,
      2,
    ) + "\n",
  );
}

function getPackageNameNoScope(name: Package) {
  const libc = name.libc == null ? "" : `-${name.libc}`;
  return `${name.os}-${name.cpu}${libc}`;
}
