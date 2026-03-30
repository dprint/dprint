#!/usr/bin/env -S deno run -A
import { createWorkflow, defineArtifact, defineMatrix, expr, job, step } from "jsr:@david/gagen@0.3.0";

const isDprintRepo = "github.repository == 'dprint/dprint'";

// === publish-cargo job ===

const cargoCheckout = step({
  name: "Checkout",
  uses: "actions/checkout@v6",
});

const cargoSetupRust = step({
  uses: "dsherret/rust-toolchain-file@v1",
}).dependsOn(cargoCheckout);

const cargoAuth = step({
  uses: "rust-lang/crates-io-auth-action@v1",
  id: "auth",
  outputs: ["token"] as const,
}).dependsOn(cargoSetupRust);

const cargoPublish = step({
  name: "Cargo publish",
  env: { CARGO_REGISTRY_TOKEN: cargoAuth.outputs.token },
  run: [
    "cd crates/dprint",
    `cargo publish ${expr("(inputs.dry_run || false) && '--dry-run' || ''")}`,
  ],
}).dependsOn(cargoAuth);

const publishCargoJob = job("publish-cargo", {
  runsOn: "ubuntu-latest",
  if: isDprintRepo,
  steps: [cargoPublish],
});

// === build-npm job ===

const npmDist = defineArtifact("npm-dist");

const buildCheckout = step({
  name: "Checkout",
  uses: "actions/checkout@v6",
});

const buildSetupDeno = step({
  uses: "denoland/setup-deno@v2",
}).comesAfter(buildCheckout);

const buildNpmPackages = step({
  name: "Build npm packages",
  run: `deno run -A deployment/npm/build.ts ${expr("inputs.version || ''")}`,
}).dependsOn(buildCheckout, buildSetupDeno);

const tarNpmDist = step({
  name: "Tar npm dist (preserves permissions)",
  run: "tar cf deployment/npm/dist.tar -C deployment/npm --exclude='node_modules' dist",
}).dependsOn(buildNpmPackages);

const uploadNpmDist = npmDist.upload({
  path: "deployment/npm/dist.tar",
  retentionDays: 1,
}).dependsOn(tarNpmDist);

const buildNpmJob = job("build-npm", {
  name: "npm build",
  runsOn: "ubuntu-latest",
  if: isDprintRepo,
  timeoutMinutes: 30,
  steps: [uploadNpmDist],
});

// === test-npm job ===

const testMatrix = defineMatrix({
  runner: ["ubuntu-latest", "macos-latest", "windows-latest"],
});

const testCheckout = step({
  name: "Checkout",
  uses: "actions/checkout@v6",
});

const installNode = step({
  name: "Install Node",
  uses: "actions/setup-node@v6",
  with: { "node-version": "24.x" },
}).dependsOn(testCheckout);

const downloadNpmDist = npmDist.download({
  dirPath: "deployment/npm",
}).dependsOn(installNode);

const extractNpmDist = step({
  name: "Extract npm dist",
  run: "tar xf deployment/npm/dist.tar -C deployment/npm",
}).dependsOn(downloadNpmDist);

const writeVerdaccioConfig = step({
  name: "Write Verdaccio config",
  run: `mkdir -p "${expr("runner.temp")}/verdaccio/storage"
cat > "${expr("runner.temp")}/verdaccio/config.yaml" << 'HEREDOC'
storage: ./storage
uplinks: {}
packages:
  '@dprint/*':
    access: $all
    publish: $all
  'dprint':
    access: $all
    publish: $all
  '**':
    access: $all
    publish: $all
max_body_size: 200mb
log: { type: stdout, format: pretty, level: warn }
HEREDOC`,
}).dependsOn(extractNpmDist);

const startVerdaccio = step({
  name: "Start Verdaccio",
  run: `npx verdaccio@6 \\
  --config "${expr("runner.temp")}/verdaccio/config.yaml" --listen 4873 &
for i in $(seq 1 30); do
  if curl -s http://localhost:4873/-/ping > /dev/null 2>&1; then
    echo "Verdaccio is ready"
    break
  fi
  sleep 1
done`,
}).dependsOn(writeVerdaccioConfig);

const configureNpm = step({
  name: "Configure npm to use Verdaccio",
  run: [
    "npm config set registry http://localhost:4873/",
    "npm config set //localhost:4873/:_authToken dummy-token",
  ],
}).dependsOn(startVerdaccio);

const publishVerdaccio = step({
  name: "Publish packages to Verdaccio",
  id: "publish-verdaccio",
  outputs: ["version"] as const,
  run: `DIST_DIR="deployment/npm/dist"
for pkg_dir in "$DIST_DIR"/@dprint/*/; do
  echo "Publishing $(basename "$pkg_dir") to Verdaccio..."
  (cd "$pkg_dir" && npm publish --registry http://localhost:4873/)
done
echo "Publishing dprint to Verdaccio..."
(cd "$DIST_DIR/dprint" && npm publish --registry http://localhost:4873/)
VERSION=$(node -p "require('./$DIST_DIR/dprint/package.json').version")
echo "version=$VERSION" >> "$GITHUB_OUTPUT"`,
}).dependsOn(configureNpm);

const testNpmInstall = step({
  name: "Test npm install dprint",
  run: `TEST_DIR="${expr("runner.temp")}/npm-test"
mkdir -p "$TEST_DIR"
cd "$TEST_DIR"
npm init -y
EXPECTED_VERSION="dprint ${expr("steps.publish-verdaccio.outputs.version")}"
npm install dprint@${expr("steps.publish-verdaccio.outputs.version")}
ACTUAL="$(npx dprint -v)"
echo "$ACTUAL"
[ "$ACTUAL" = "$EXPECTED_VERSION" ] || { echo "Version mismatch: expected '$EXPECTED_VERSION', got '$ACTUAL'"; exit 1; }`,
}).dependsOn(publishVerdaccio);

const testBinCjs = step({
  name: "Test dprint via bin.cjs fallback",
  run: [
    `cd "${expr("runner.temp")}/npm-test"`,
    "node node_modules/dprint/bin.cjs -v",
  ],
}).dependsOn(testNpmInstall);

const testReadonly = step({
  name: "Test dprint via simulated readonly file system",
  run: `TEST_DIR="${expr("runner.temp")}/readonly-test"
mkdir -p "$TEST_DIR"
cd "$TEST_DIR"
npm init -y
npm install dprint@${expr("steps.publish-verdaccio.outputs.version")}
rm -f node_modules/dprint/dprint*
DPRINT_SIMULATED_READONLY_FILE_SYSTEM=1 node node_modules/dprint/bin.cjs -v`,
}).dependsOn(testBinCjs);

const testGlobalInstall = step({
  name: "Test npm global install dprint",
  run: `EXPECTED_VERSION="dprint ${expr("steps.publish-verdaccio.outputs.version")}"
npm install -g dprint@${expr("steps.publish-verdaccio.outputs.version")}
ACTUAL="$(dprint -v)"
echo "$ACTUAL"
[ "$ACTUAL" = "$EXPECTED_VERSION" ] || { echo "Version mismatch: expected '$EXPECTED_VERSION', got '$ACTUAL'"; exit 1; }
# verify the global bin entry points directly at the native binary, not bin.cjs
if [ "$RUNNER_OS" = "Windows" ]; then
  DPRINT_CMD="$(npm prefix -g)/dprint.cmd"
  grep -q "bin.cjs" "$DPRINT_CMD" && { echo "ERROR: dprint.cmd still points to bin.cjs"; exit 1; }
  echo "dprint.cmd correctly points to native binary"
else
  DPRINT_LINK="$(which dprint)"
  LINK_TARGET="$(readlink "$DPRINT_LINK")"
  echo "dprint symlink target: $LINK_TARGET"
  echo "$LINK_TARGET" | grep -q "bin.cjs" && { echo "ERROR: dprint symlink still points to bin.cjs"; exit 1; }
  echo "dprint symlink correctly points to native binary"
fi
npm uninstall -g dprint`,
}).dependsOn(testReadonly);

const testGlobalInstallIgnoreScripts = step({
  name: "Test npm global install dprint (--ignore-scripts)",
  run: `EXPECTED_VERSION="dprint ${expr("steps.publish-verdaccio.outputs.version")}"
npm install -g --ignore-scripts dprint@${expr("steps.publish-verdaccio.outputs.version")}
ACTUAL="$(dprint -v)"
echo "$ACTUAL"
[ "$ACTUAL" = "$EXPECTED_VERSION" ] || { echo "Version mismatch: expected '$EXPECTED_VERSION', got '$ACTUAL'"; exit 1; }
npm uninstall -g dprint`,
}).dependsOn(testGlobalInstall);

const testNpmJob = job("test-npm", {
  name: `npm test (${testMatrix.runner})`,
  needs: [buildNpmJob],
  runsOn: testMatrix.runner,
  timeoutMinutes: 15,
  strategy: { matrix: testMatrix, failFast: false },
  defaults: { run: { shell: "bash" } },
  steps: [testGlobalInstallIgnoreScripts],
});

// === publish-npm job ===

const pubCheckout = step({
  name: "Checkout",
  uses: "actions/checkout@v6",
});

const pubSetupDeno = step({
  uses: "denoland/setup-deno@v2",
}).dependsOn(pubCheckout);

const pubInstallNode = step({
  name: "Install Node",
  uses: "actions/setup-node@v6",
  with: {
    "node-version": "24.x",
    "registry-url": "https://registry.npmjs.org",
  },
}).dependsOn(pubSetupDeno);

const pubDownloadNpmDist = npmDist.download({
  dirPath: "deployment/npm",
}).dependsOn(pubInstallNode);

const pubExtractNpmDist = step({
  name: "Extract npm dist",
  run: "tar xf deployment/npm/dist.tar -C deployment/npm",
}).dependsOn(pubDownloadNpmDist);

const pubPublishNpm = step({
  name: "Publish to npm",
  run: `deno run -A deployment/npm/build.ts ${expr("inputs.version || ''")} --publish-only`,
}).dependsOn(pubExtractNpmDist, pubSetupDeno);

const publishNpmJob = job("publish-npm", {
  name: "npm publish",
  needs: [buildNpmJob, testNpmJob],
  if: "!(inputs.dry_run || false)",
  runsOn: "ubuntu-latest",
  timeoutMinutes: 15,
  permissions: { "id-token": "write" },
  steps: [pubPublishNpm],
});

// === generate ===

createWorkflow({
  name: "Package Publish",
  on: {
    release: { types: ["published"] },
    workflow_dispatch: {
      inputs: {
        version: {
          description: "Version",
          type: "string",
        },
        dry_run: {
          description: "Dry run (build and test npm, but skip publishing)",
          type: "boolean",
          default: true,
        },
      },
    },
  },
  permissions: { "id-token": "write", contents: "read" },
  jobs: [
    publishCargoJob,
    buildNpmJob,
    testNpmJob,
    publishNpmJob,
  ],
}).writeOrLint({
  filePath: new URL("./publish.yml", import.meta.url),
  header: "# GENERATED BY ./publish.generate.ts -- DO NOT DIRECTLY EDIT",
});
