#!/usr/bin/env -S deno run -A
import { createWorkflow, defineArtifact, defineMatrix, expr, job, step } from "jsr:@david/gagen@0.3.0";

const isDprintRepo = "github.repository == 'dprint/dprint'";
const npmDist = defineArtifact("npm-dist");

// === publish-cargo job ===

const publishCargoJob = job("publish-cargo", {
  runsOn: "ubuntu-latest",
  if: isDprintRepo,
  steps: step(
    { name: "Checkout", uses: "actions/checkout@v6" },
    { uses: "dsherret/rust-toolchain-file@v1" },
    { uses: "rust-lang/crates-io-auth-action@v1", id: "auth" },
    {
      name: "Cargo publish",
      env: { CARGO_REGISTRY_TOKEN: expr("steps.auth.outputs.token") },
      run: [
        "cd crates/dprint",
        `cargo publish ${expr("(inputs.dry_run || false) && '--dry-run' || ''")}`,
      ],
    },
  ),
});

// === build-npm job ===

const buildNpmJob = job("build-npm", {
  name: "npm build",
  runsOn: "ubuntu-latest",
  if: isDprintRepo,
  timeoutMinutes: 30,
  steps: step(
    { name: "Checkout", uses: "actions/checkout@v6" },
    { uses: "denoland/setup-deno@v2" },
    { name: "Build npm packages", run: `deno run -A deployment/npm/build.ts ${expr("inputs.version || ''")}` },
    { name: "Tar npm dist (preserves permissions)", run: "tar cf deployment/npm/dist.tar -C deployment/npm --exclude='node_modules' dist" },
    npmDist.upload({ path: "deployment/npm/dist.tar", retentionDays: 1 }),
  ),
});

// === test-npm job ===

const testMatrix = defineMatrix({
  runner: ["ubuntu-latest", "macos-latest", "windows-latest"],
});

const testNpmJob = job("test-npm", {
  name: `npm test (${testMatrix.runner})`,
  needs: [buildNpmJob],
  runsOn: testMatrix.runner,
  timeoutMinutes: 15,
  strategy: { matrix: testMatrix, failFast: false },
  defaults: { run: { shell: "bash" } },
  steps: step(
    { name: "Checkout", uses: "actions/checkout@v6" },
    { name: "Install Node", uses: "actions/setup-node@v6", with: { "node-version": "24.x" } },
    npmDist.download({ dirPath: "deployment/npm" }),
    { name: "Extract npm dist", run: "tar xf deployment/npm/dist.tar -C deployment/npm" },
    {
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
    },
    {
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
    },
    {
      name: "Configure npm to use Verdaccio",
      run: [
        "npm config set registry http://localhost:4873/",
        "npm config set //localhost:4873/:_authToken dummy-token",
      ],
    },
    {
      name: "Publish packages to Verdaccio",
      id: "publish-verdaccio",
      run: `DIST_DIR="deployment/npm/dist"
for pkg_dir in "$DIST_DIR"/@dprint/*/; do
  echo "Publishing $(basename "$pkg_dir") to Verdaccio..."
  (cd "$pkg_dir" && npm publish --registry http://localhost:4873/)
done
echo "Publishing dprint to Verdaccio..."
(cd "$DIST_DIR/dprint" && npm publish --registry http://localhost:4873/)
VERSION=$(node -p "require('./$DIST_DIR/dprint/package.json').version")
echo "version=$VERSION" >> "$GITHUB_OUTPUT"`,
    },
    {
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
    },
    {
      name: "Test dprint via bin.cjs fallback",
      run: [
        `cd "${expr("runner.temp")}/npm-test"`,
        "node node_modules/dprint/bin.cjs -v",
      ],
    },
    {
      name: "Test dprint via simulated readonly file system",
      run: `TEST_DIR="${expr("runner.temp")}/readonly-test"
mkdir -p "$TEST_DIR"
cd "$TEST_DIR"
npm init -y
npm install dprint@${expr("steps.publish-verdaccio.outputs.version")}
rm -f node_modules/dprint/dprint*
DPRINT_SIMULATED_READONLY_FILE_SYSTEM=1 node node_modules/dprint/bin.cjs -v`,
    },
    {
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
    },
    {
      name: "Test npm global install dprint (--ignore-scripts)",
      run: `EXPECTED_VERSION="dprint ${expr("steps.publish-verdaccio.outputs.version")}"
npm install -g --ignore-scripts dprint@${expr("steps.publish-verdaccio.outputs.version")}
ACTUAL="$(dprint -v)"
echo "$ACTUAL"
[ "$ACTUAL" = "$EXPECTED_VERSION" ] || { echo "Version mismatch: expected '$EXPECTED_VERSION', got '$ACTUAL'"; exit 1; }
npm uninstall -g dprint`,
    },
  ),
});

// === publish-npm job ===

const publishNpmJob = job("publish-npm", {
  name: "npm publish",
  needs: [buildNpmJob, testNpmJob],
  if: "!(inputs.dry_run || false)",
  runsOn: "ubuntu-latest",
  timeoutMinutes: 15,
  permissions: { "id-token": "write" },
  steps: step(
    { name: "Checkout", uses: "actions/checkout@v6" },
    { uses: "denoland/setup-deno@v2" },
    { name: "Install Node", uses: "actions/setup-node@v6", with: { "node-version": "24.x", "registry-url": "https://registry.npmjs.org" } },
    npmDist.download({ dirPath: "deployment/npm" }),
    { name: "Extract npm dist", run: "tar xf deployment/npm/dist.tar -C deployment/npm" },
    { name: "Publish to npm", run: `deno run -A deployment/npm/build.ts ${expr("inputs.version || ''")} --publish-only` },
  ),
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
