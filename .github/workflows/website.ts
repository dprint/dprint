#!/usr/bin/env -S deno run -A
import { artifact, expr, job, step, workflow } from "jsr:@david/gagen@0.6.0";

const isDeploy = "github.event_name == 'release' || (github.event_name == 'workflow_dispatch' && inputs.deploy)";
// the built site and bundled worker, which the deploy uploads
const deployment = artifact("website-deployment", { retentionDays: 1 });

const buildJob = job("build", {
  runsOn: "ubuntu-latest",
  permissions: { contents: "read" },
  steps: step(
    { name: "Checkout", uses: "actions/checkout@v7", with: { "persist-credentials": false } },
    // CI doesn't run for docs only changes, so their formatting is checked here.
    // Rust files are excluded because they're formatted with rustfmt, which
    // isn't installed (CI checks them).
    { uses: "dprint/check@v2", with: { cache: true, args: "--excludes '**/*.rs'" } },
    { uses: "denoland/setup-deno@v2", with: { cache: true, "deno-version": "canary" } },
    { name: "Install dependencies", run: "(cd website && deno install && cd wrangler && deno install)" },
    { name: "Build playground", run: "(cd website/playground && deno task build)" },
    { name: "Test playground", run: "(cd website/playground && deno task test)" },
    { name: "Test worker", run: "(cd website && deno task test)" },
    { name: "Build website", run: "(cd website && deno task build:ci)" },
    // bundled for every PR, so a worker that doesn't bundle fails before a deploy
    { name: "Bundle worker", run: "(cd website && deno task worker:bundle)" },
    {
      name: "Prepare deployment",
      if: isDeploy,
      // a directory that isn't in the repo (deployment/ is the cli's)
      run: "mkdir website-deployment && cp -r website/_site website/dist website-deployment/",
    },
    deployment.upload({ path: "website-deployment" }).if(isDeploy),
  ),
});

// Deploying is the only step that needs the Cloudflare secrets, so it runs in
// its own job and the build job (which runs for every PR) never has them. The
// job doesn't build anything. It uploads the site and worker the build job
// built, with the wrangler config and lockfile from the commit.
const deployJob = job("deploy", {
  runsOn: "ubuntu-latest",
  needs: [buildJob],
  if: isDeploy,
  permissions: { contents: "read" },
  // has the cloudflare secrets
  environment: { name: "website", url: "https://dprint.dev" },
  steps: step(
    {
      name: "Checkout wrangler config",
      uses: "actions/checkout@v7",
      with: {
        "persist-credentials": false,
        "sparse-checkout": "website/wrangler.toml\nwebsite/wrangler/",
        "sparse-checkout-cone-mode": false,
      },
    },
    { uses: "denoland/setup-deno@v2", with: { "deno-version": "v2.9.6" } },
    deployment.download({ dirPath: "website" }),
    // installs only wrangler's dependencies, from the frozen lockfile
    { name: "Install wrangler", run: "(cd website/wrangler && deno install)" },
    {
      name: "Deploy",
      // uploads the worker bundled by the build job as is. wrangler is run
      // directly instead of with its bin script, which starts another process
      // that isn't limited by these permissions. it only needs to run esbuild,
      // which it starts when it's loaded, so it runs a copy of esbuild outside
      // the directories it can write to
      run: [
        "cd website",
        "esbuild=(\"$PWD\"/wrangler/node_modules/.deno/@esbuild+linux-x64@*/node_modules/@esbuild/linux-x64/bin/esbuild)",
        "if [ ${#esbuild[@]} -ne 1 ] || [ ! -f \"$esbuild\" ]; then echo \"Expected one esbuild binary: ${esbuild[*]}\"; exit 1; fi",
        "install -m 555 \"$esbuild\" \"$RUNNER_TEMP/esbuild\"",
        "export ESBUILD_BINARY_PATH=\"$RUNNER_TEMP/esbuild\" XDG_CONFIG_HOME=\"$PWD/.config\" WRANGLER_SEND_METRICS=false",
        [
          "deno run --config wrangler/deno.json",
          "--allow-read=.,\"$HOME/.wrangler\",\"$RUNNER_TEMP/esbuild\" --allow-write=.config,.wrangler",
          "--allow-env --allow-sys --allow-net=api.cloudflare.com --allow-run=\"$RUNNER_TEMP/esbuild\"",
          "wrangler/node_modules/wrangler/wrangler-dist/cli.js deploy dist/main.js --no-bundle",
        ].join(" "),
      ],
      env: {
        CLOUDFLARE_API_TOKEN: expr("secrets.CLOUDFLARE_API_TOKEN"),
        CLOUDFLARE_ACCOUNT_ID: expr("secrets.CLOUDFLARE_ACCOUNT_ID"),
      },
    },
  ),
});

// What the website is built from, plus the docs and markdown files that CI
// skips so their formatting still gets checked.
const paths = [
  "website/**",
  "docs/**",
  "**.md",
  // copied into the website by website/_config.ts
  "crates/dprint/src/commands/lsp/config_schema.json",
  "dprint.json",
  ".github/workflows/website.*",
];

workflow({
  name: "Website",
  on: {
    workflow_dispatch: {
      inputs: {
        deploy: {
          description: "Deploy to Cloudflare",
          type: "boolean",
          default: false,
        },
      },
    },
    // deploy when the drafted release is published
    release: { types: ["published"] },
    push: { branches: ["main"], paths },
    pull_request: { branches: ["main"], paths },
  },
  permissions: {},
  jobs: [buildJob, deployJob],
}).writeOrLint({
  filePath: new URL("./website.generated.yml", import.meta.url),
  header: "# GENERATED BY ./website.ts -- DO NOT DIRECTLY EDIT",
});
