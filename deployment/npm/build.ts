import $ from "https://deno.land/x/dax@0.32.0/mod.ts";

interface Package {
  zipFileName: string;
  os: string;
  cpu: string;
}

const packages: Package[] = [{
  zipFileName: "dprint-x86_64-pc-windows-msvc.zip",
  os: "windows",
  cpu: "x64",
}];

const markdownText = `# dprint

npm CLI distribution for [dprint](https://dprint.dev)—a pluggable and configurable code formatting platform.
`;

const currentDir = $.path(import.meta).parentOrThrow();
const repoRootDir = currentDir.parentOrThrow();
const outputDir = currentDir.join("./dist");
const dprintDir = outputDir.join("dprint");

outputDir.removeSync({ recursive: true });
dprintDir.mkdirSync({ recursive: true });

const version = Deno.args[0];

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
};
currentDir.join("bin.js").copyFileSync(dprintDir.join("bin.js"));
dprintDir.join("package.json")
  .writeTextSync(JSON.stringify(pkgJson, undefined, 2) + "\n");
repoRootDir.join("LICENSE").copyFileSync(dprintDir.join("LICENSE"));
dprintDir.join("README.md").writeTextSync(markdownText);
