import $ from "https://deno.land/x/dax@0.33.0/mod.ts";
// @ts-types="npm:@types/decompress@4.2.7"
import decompress from "npm:decompress@4.2.1";

interface Package {
  zipFileName: string;
  os: "win32" | "darwin" | "linux";
  cpu: "x64" | "arm64" | "riscv64";
  libc?: "glibc" | "musl";
}

const packages: Package[] = [{
  zipFileName: "dprint-x86_64-pc-windows-msvc.zip",
  os: "win32",
  cpu: "x64",
}, {
  // use x64_64 until there's an arm64 build
  zipFileName: "dprint-x86_64-pc-windows-msvc.zip",
  os: "win32",
  cpu: "arm64",
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
}, {
  zipFileName: "dprint-aarch64-unknown-linux-musl.zip",
  os: "linux",
  cpu: "arm64",
  libc: "musl",
}, {
  zipFileName: "dprint-riscv64gc-unknown-linux-gnu.zip",
  os: "linux",
  cpu: "riscv64",
  libc: "glibc",
}];

const markdownText = `# dprint

npm CLI distribution for [dprint](https://dprint.dev)—a pluggable and configurable code formatting platform.
`;

const currentDir = $.path(import.meta).parentOrThrow();
const rootDir = currentDir.parentOrThrow().parentOrThrow();
const outputDir = currentDir.join("./dist");
const scopeDir = outputDir.join("@dprint");
const dprintDir = outputDir.join("dprint");
const version = resolveVersion();

$.logStep(`Publishing ${version}...`);

await $`rm -rf ${outputDir}`;
await $`mkdir -p ${dprintDir} ${scopeDir}`;

// setup dprint packages
{
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
    // for yarn berry (https://github.com/dprint/dprint/issues/686)
    "preferUnplugged": true,
    "scripts": {
      "postinstall": "node ./install.js",
    },
    optionalDependencies: packages
      .map(pkg => `@dprint/${getPackageNameNoScope(pkg)}`)
      .reduce((obj, pkgName) => ({ ...obj, [pkgName]: version }), {}),
  };
  currentDir.join("bin.js").copyFileToDirSync(dprintDir);
  currentDir.join("install_api.js").copyFileToDirSync(dprintDir);
  currentDir.join("install.js").copyFileToDirSync(dprintDir);
  dprintDir.join("package.json").writeJsonPrettySync(pkgJson);
  rootDir.join("LICENSE").copyFileSync(dprintDir.join("LICENSE"));
  dprintDir.join("README.md").writeTextSync(markdownText);
  // ensure the test files don't get published
  dprintDir.join(".npmignore").writeTextSync("dprint\ndprint.exe\n");

  // setup each binary package
  for (const pkg of packages) {
    const pkgName = getPackageNameNoScope(pkg);
    $.logStep(`Setting up @dprint/${pkgName}...`);
    const pkgDir = scopeDir.join(pkgName);
    const zipPath = pkgDir.join("output.zip");

    await $`mkdir -p ${pkgDir}`;

    // download and extract the zip file
    const zipUrl = `https://github.com/dprint/dprint/releases/download/${version}/${pkg.zipFileName}`;
    await $.request(zipUrl).showProgress().pipeToPath(zipPath);
    await decompress(zipPath.toString(), pkgDir.toString());
    zipPath.removeSync();

    // create the package.json and readme
    pkgDir.join("README.md").writeTextSync(`# @dprint/${pkgName}\n\n${pkgName} distribution of dprint.\n`);
    pkgDir.join("package.json").writeJsonPrettySync({
      "name": `@dprint/${pkgName}`,
      "version": version,
      "description": `${pkgName} distribution of the dprint code formatter`,
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
    });
  }
}

// verify that the package is created correctly
{
  $.logStep("Verifying packages...");
  const testPlatform = Deno.build.os == "windows"
    ? (Deno.build.arch === "x86_64" ? "@dprint/win32-x64" : "@dprint/win32-arm64")
    : Deno.build.os === "darwin"
    ? (Deno.build.arch === "x86_64" ? "@dprint/darwin-x64" : "@dprint/darwin-arm64")
    : "@dprint/linux-x64-glibc";
  outputDir.join("package.json").writeJsonPrettySync({
    workspaces: [
      "dprint",
      // There seems to be a bug with npm workspaces where this doesn't
      // work, so for now make some assumptions and only include the package
      // that works on the CI for the current operating system
      // ...packages.map(p => `@dprint/${getPackageNameNoScope(p)}`),
      testPlatform,
    ],
  });

  const dprintExe = Deno.build.os === "windows" ? "dprint.exe" : "dprint";
  await $`npm install`.cwd(dprintDir);

  // ensure the post-install script adds the executable to the dprint package,
  // which is necessary for faster caching and to ensure the vscode extension
  // picks it up
  if (!dprintDir.join(dprintExe).existsSync()) {
    throw new Error("dprint executable did not exist after post install");
  }

  // run once after post install created dprint, once with a simulated readonly file system, once creating the cache and once with
  await $`node bin.js -v && rm ${dprintExe} && DPRINT_SIMULATED_READONLY_FILE_SYSTEM=1 node bin.js -v && node bin.js -v && node bin.js -v`.cwd(dprintDir);

  if (!dprintDir.join(dprintExe).existsSync()) {
    throw new Error("dprint executable did not exist when lazily initialized");
  }
}

// publish if necessary
if (Deno.args.includes("--publish")) {
  for (const pkg of packages) {
    const pkgName = getPackageNameNoScope(pkg);
    $.logStep(`Publishing @dprint/${pkgName}...`);
    if (await checkPackagePublished(`@dprint/${pkgName}`)) {
      $.logLight("  Already published.");
      continue;
    }
    const pkgDir = scopeDir.join(pkgName);
    await $`cd ${pkgDir} && npm publish --access public`;
  }

  $.logStep(`Publishing dprint...`);
  await $`cd ${dprintDir} && npm publish --access public`;
}

function getPackageNameNoScope(name: Package) {
  const libc = name.libc == null ? "" : `-${name.libc}`;
  return `${name.os}-${name.cpu}${libc}`;
}

function resolveVersion() {
  if (Deno.args[0] != null && /^[0-9]+\.[0-9]+\.[0-9]+/.test(Deno.args[0])) {
    return Deno.args[0];
  }
  const version = (rootDir.join("crates/dprint/Cargo.toml").readTextSync().match(/version = "(.*?)"/))?.[1];
  if (version == null) {
    throw new Error("Could not resolve version.");
  }
  return version;
}

async function checkPackagePublished(pkgName: string) {
  const result = await $`npm info ${pkgName}@${version}`.quiet().noThrow();
  return result.code === 0;
}
