#!/usr/bin/env -S deno run -A
import $ from "jsr:@david/dax@0.45.0";
import { conditions, defineMatrix, expr, type ExpressionValue, isLinting, job, step, workflow } from "jsr:@david/gagen@0.5.0";

enum OperatingSystem {
  Mac = "macOS-latest",
  MacX86 = "macos-15-intel",
  Windows = "windows-latest",
  WindowsArm = "windows-11-arm",
  Linux = "ubuntu-22.04",
  LinuxArm = "ubuntu-24.04-arm",
}

interface ProfileData {
  os: OperatingSystem;
  target: string;
  runTests?: boolean;
  /** Build using cross. */
  cross?: boolean;
  /**
   * Build by running cargo directly inside this Docker image. Used for targets
   * cross doesn't provide an image for (e.g. powerpc64le musl), where the image
   * already bundles the toolchain.
   */
  muslCrossImage?: string;
}

const profileDataItems: ProfileData[] = [{
  os: OperatingSystem.MacX86,
  target: "x86_64-apple-darwin",
  runTests: true,
}, {
  os: OperatingSystem.Mac,
  target: "aarch64-apple-darwin",
  runTests: true,
}, {
  os: OperatingSystem.Windows,
  target: "x86_64-pc-windows-msvc",
  runTests: true,
}, {
  os: OperatingSystem.WindowsArm,
  target: "aarch64-pc-windows-msvc",
  runTests: true,
}, {
  os: OperatingSystem.Linux,
  target: "x86_64-unknown-linux-gnu",
  runTests: true,
}, {
  os: OperatingSystem.Linux,
  target: "x86_64-unknown-linux-musl",
}, {
  os: OperatingSystem.LinuxArm,
  target: "aarch64-unknown-linux-gnu",
  runTests: true,
}, {
  os: OperatingSystem.LinuxArm,
  target: "aarch64-unknown-linux-musl",
}, {
  os: OperatingSystem.Linux,
  target: "riscv64gc-unknown-linux-gnu",
  cross: true,
}, {
  os: OperatingSystem.Linux,
  target: "loongarch64-unknown-linux-gnu",
  cross: true,
}, {
  os: OperatingSystem.Linux,
  target: "loongarch64-unknown-linux-musl",
  cross: true,
}, {
  // ppc64le: built with cross. Cranelift has no native ppc64 backend, so this
  // compiles to wasmtime's portable Pulley bytecode (see the `use_pulley` cfg in
  // crates/dprint/build.rs).
  os: OperatingSystem.Linux,
  target: "powerpc64le-unknown-linux-gnu",
  cross: true,
}, {
  // cross has no powerpc64le musl image, so build directly in the prebuilt
  // rust-musl-cross toolchain image instead (see the musl image build step).
  os: OperatingSystem.Linux,
  target: "powerpc64le-unknown-linux-musl",
  muslCrossImage: "ghcr.io/rust-cross/rust-musl-cross:powerpc64le-musl",
}, {
  // android (Termux): built with cross using its built-in NDK image. The
  // sandbox lacks the signal-based trap handling native code relies on, so it
  // compiles to wasmtime's portable Pulley bytecode (see the `use_pulley` cfg in
  // crates/dprint/build.rs).
  os: OperatingSystem.Linux,
  target: "aarch64-linux-android",
  cross: true,
}, {
  os: OperatingSystem.Linux,
  target: "x86_64-linux-android",
  cross: true,
}];

const profiles = profileDataItems.map(profile => {
  return {
    ...profile,
    zipChecksumEnvVarName: `ZIP_CHECKSUM_${profile.target.toUpperCase().replaceAll("-", "_")}`,
    get installerChecksumEnvVarName() {
      if (profile.target !== "x86_64-pc-windows-msvc") {
        throw new Error("Check for windows x86_64 before accessing.");
      }
      return `INSTALLER_CHECKSUM_${profile.target.toUpperCase().replaceAll("-", "_")}`;
    },
    artifactsName: `${profile.target}-artifacts`,
    zipFileName: `dprint-${profile.target}.zip`,
    get installerFileName() {
      if (profile.target !== "x86_64-pc-windows-msvc") {
        throw new Error("Check for windows x86_64 before accessing.");
      }
      return `dprint-${profile.target}-installer.exe`;
    },
  };
});

const isTag = conditions.isTag();
const isNotTag = isTag.not();

const matrix = defineMatrix({
  include: profileDataItems.map(profile => ({
    os: profile.os as string,
    run_tests: (profile.runTests ?? false).toString(),
    target: profile.target,
    cross: (profile.cross ?? false).toString(),
    musl_image: profile.muslCrossImage ?? "",
  })),
});

const runTests = matrix.run_tests.equals("true");
const runDebugTests = runTests.and(isNotTag);
const isCross = matrix.cross.equals("true");
const isMuslImage = matrix.musl_image.notEquals("");
const isLinuxGnu = matrix.target.equals("x86_64-unknown-linux-gnu");

// === build job ===

const checkout = step({ name: "Checkout", uses: "actions/checkout@v6" });
const setupDeno = step({
  uses: "denoland/setup-deno@v2",
  with: {
    "deno-version": "canary",
  },
});
const setupRust = step({
  uses: "dsherret/rust-toolchain-file@v1",
}, {
  uses: "Swatinem/rust-cache@v2",
  with: { key: matrix.target },
}, {
  name: "Setup (Linux x86_64-musl)",
  if: matrix.target.equals("x86_64-unknown-linux-musl"),
  run: [
    "sudo apt update",
    "sudo apt install musl musl-dev musl-tools",
    "rustup target add x86_64-unknown-linux-musl",
  ],
}, {
  name: "Setup (Linux aarch64)",
  if: matrix.target.equals("aarch64-unknown-linux-gnu"),
  run: [
    "sudo apt update",
    "sudo apt install gcc-aarch64-linux-gnu",
    "rustup target add aarch64-unknown-linux-gnu",
  ],
}, {
  name: "Setup (Linux aarch64-musl)",
  if: matrix.target.equals("aarch64-unknown-linux-musl"),
  run: [
    "sudo apt update",
    "sudo apt install musl musl-dev musl-tools",
    "rustup target add aarch64-unknown-linux-musl",
  ],
}, {
  name: "Setup cross",
  if: isCross,
  run: "cargo install cross --git https://github.com/cross-rs/cross --rev 36c0d7810ddde073f603c82d896c2a6c886ff7a4",
}).dependsOn(checkout).comesAfter(setupDeno);

const lint = step.if(isLinuxGnu.and(isNotTag))(
  step({
    name: "Clippy",
    run: "cargo clippy",
  }).dependsOn(setupRust),
  step({
    uses: "dprint/check@v2.3",
  }).dependsOn(setupDeno),
  step({
    name: "Lint CI Generation",
    run: [
      "./.github/workflows/ci.ts --lint",
      "./.github/workflows/publish.ts --lint",
      "./.github/workflows/publish_crate_core.ts --lint",
      "./.github/workflows/publish_crate_core-macros.ts --lint",
      "./.github/workflows/publish_crate_dev.ts --lint",
      "./.github/workflows/website.ts --lint",
      "./.github/workflows/release.ts --lint",
    ],
  }).dependsOn(setupDeno),
);

const aarch64LinkerEnv = {
  CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER: "aarch64-linux-gnu-gcc",
};

// Builds inside the rust-musl-cross image, which bundles the toolchain and runs
// as root (so it can install the pinned toolchain into its own /root/.rustup --
// the reason this can't go through cross, which runs as the non-root host user).
// The build output is then chown'd back so later steps can read/zip it.
function muslImageRun(releaseArgs: string): string[] {
  // the image bundles rust-std for its own toolchain, but rust-toolchain.toml
  // pins a different one, so add the target's std to the pinned toolchain first
  const build = `rustup target add ${matrix.target} && cargo build -p dprint --locked --target ${matrix.target}${releaseArgs}`;
  return [
    `docker run --rm -v "$GITHUB_WORKSPACE":/home/rust/src -w /home/rust/src ${matrix.musl_image} bash -c "${build}"`,
    `sudo chown -R "$(id -u):$(id -g)" "$GITHUB_WORKSPACE/target"`,
  ];
}

const buildDebug = step({
  name: "Build (Debug)",
  if: isCross.not().and(isMuslImage.not()),
  env: aarch64LinkerEnv,
  run: `cargo build -p dprint --locked --target ${matrix.target}`,
}, {
  name: "Build cross (Debug)",
  if: isCross,
  run: `cross build -p dprint --locked --target ${matrix.target}`,
}, {
  name: "Build musl image (Debug)",
  if: isMuslImage,
  run: muslImageRun(""),
}).dependsOn(setupRust);
const buildRelease = step({
  name: "Build (Release)",
  if: isCross.not().and(isMuslImage.not()),
  env: aarch64LinkerEnv,
  run: `cargo build -p dprint --locked --target ${matrix.target} --release`,
}, {
  name: "Build cross (Release)",
  if: isCross,
  run: `cross build -p dprint --locked --target ${matrix.target} --release`,
}, {
  name: "Build musl image (Release)",
  if: isMuslImage,
  run: muslImageRun(" --release"),
}).dependsOn(setupRust);

const tests = step(
  // debug
  step.if(runDebugTests).dependsOn(buildDebug)(
    step({
      name: "Build test plugins (Debug)",
      run: `cargo build -p test-process-plugin --locked --target ${matrix.target}`,
    }),
    step({
      name: "Test (Debug)",
      run: `cargo test --locked --target ${matrix.target} --all-features`,
    }),
    step({
      name: "Test integration",
      if: isLinuxGnu,
      run: `cargo run -p dprint --locked --target ${matrix.target} -- check`,
    }),
  ),
  // release
  step.if(runTests.and(isTag)).dependsOn(buildRelease)(
    step({
      name: "Build test plugins (Release)",
      run: `cargo build -p test-process-plugin --locked --target ${matrix.target} --release`,
    }),
    step({
      name: "Test (Release)",
      run: `cargo test --locked --target ${matrix.target} --all-features --release`,
    }),
  ),
);

const createInstaller = step.dependsOn(buildRelease)({
  name: "Create installer (Windows x86_64)",
  uses: "joncloud/makensis-action@v2.0",
  if: matrix.target.equals("x86_64-pc-windows-msvc").and(isTag),
  with: { "script-file": `${expr("github.workspace")}/deployment/installer/dprint-installer.nsi` },
});

function getPreReleaseStepForProfile(profile: typeof profiles[0]) {
  function getRunstep(): string[] {
    switch (profile.os) {
      case OperatingSystem.Mac:
      case OperatingSystem.MacX86:
      case OperatingSystem.Linux:
      case OperatingSystem.LinuxArm:
        return [
          `cd target/${profile.target}/release`,
          `zip -r ${profile.zipFileName} dprint`,
          `echo "ZIP_CHECKSUM=$(shasum -a 256 ${profile.zipFileName} | awk '{print $1}')" >> $GITHUB_OUTPUT`,
        ];
      case OperatingSystem.WindowsArm:
      case OperatingSystem.Windows: {
        const installerSteps = profile.target === "x86_64-pc-windows-msvc"
          ? [
            `mv deployment/installer/${profile.installerFileName} target/${profile.target}/release/${profile.installerFileName}`,
            `echo "INSTALLER_CHECKSUM=$(sha256sum target/${profile.target}/release/${profile.installerFileName} | awk '{print $1}')" >> $GITHUB_OUTPUT`,
          ]
          : [];
        return [
          `(cd target/${profile.target}/release && 7z a -mx9 ${profile.zipFileName} dprint.exe)`,
          `echo "ZIP_CHECKSUM=$(sha256sum target/${profile.target}/release/${profile.zipFileName} | awk '{print $1}')" >> $GITHUB_OUTPUT`,
          ...installerSteps,
        ];
      }
      default: {
        const _assertNever: never = profile.os;
        throw new Error(`Unhandled OS: ${profile.os}`);
      }
    }
  }

  const result = step({
    name: `Pre-release (${profile.target})`,
    id: `pre_release_${profile.target.replaceAll("-", "_")}`,
    run: getRunstep(),
    outputs: ["ZIP_CHECKSUM", "INSTALLER_CHECKSUM"] as const,
  }).dependsOn(buildRelease);
  if (profile.os === OperatingSystem.Windows) {
    return result.dependsOn(createInstaller);
  } else {
    return result;
  }
}

const buildJobOutputs: Record<string, ExpressionValue> = {};
const uploadArtifacts = step(...profiles.map((profile) => {
  const paths = [
    `target/${profile.target}/release/${profile.zipFileName}`,
  ];
  if (profile.target === "x86_64-pc-windows-msvc") {
    paths.push(
      `target/${profile.target}/release/${profile.installerFileName}`,
    );
  }
  const preReleaseStep = getPreReleaseStepForProfile(profile);
  buildJobOutputs[profile.zipChecksumEnvVarName] = preReleaseStep.outputs.ZIP_CHECKSUM;
  if (profile.target === "x86_64-pc-windows-msvc") {
    buildJobOutputs[profile.installerChecksumEnvVarName] = preReleaseStep.outputs.INSTALLER_CHECKSUM;
  }
  return step.dependsOn(preReleaseStep)({
    name: `Upload artifacts (${profile.target})`,
    if: matrix.target.equals(profile.target).and(isTag),
    uses: "actions/upload-artifact@v6",
    with: {
      name: profile.artifactsName,
      path: paths.join("\n"),
    },
  });
}));

const installerTests = step.if(runDebugTests)(
  {
    name: "Test shell installer",
    run: [
      "cd website/src/assets",
      "chmod +x install.sh",
      "./install.sh",
    ],
  },
  {
    name: "Test powershell installer (Windows)",
    if: matrix.target.equals("x86_64-pc-windows-msvc"),
    shell: "pwsh",
    run: ["cd website/src/assets", "./install.ps1"],
  },
  // TODO: re-enable after the next release. build.ts downloads the release
  // artifacts for the given version, and the new powerpc64le zips won't exist
  // until a release includes them.
  // step({
  //   name: "Test npm",
  //   run: [
  //     "cd deployment/npm",
  //     "deno run -A build.ts 0.51.0",
  //   ],
  // }).dependsOn(setupDeno),
);

const buildJob = job("build", {
  name: matrix.target,
  runsOn: matrix.os,
  strategy: { matrix },
  defaults: { run: { shell: "bash" } },
  env: {
    // disabled to reduce ./target size and generally it's slower enabled
    CARGO_INCREMENTAL: 0,
    RUST_BACKTRACE: "full",
  },
  steps: step.if(
    matrix.target.notEquals("aarch64-unknown-linux-gnu")
      .and(matrix.target.notEquals("aarch64-unknown-linux-musl"))
      .or(conditions.isBranch("main"))
      .or(isTag),
  )(
    lint,
    buildDebug,
    tests,
    uploadArtifacts,
    installerTests,
  ),
  outputs: buildJobOutputs,
});

// === draft_release job ===

const draftReleaseJob = job("draft_release", {
  name: "draft_release",
  runsOn: "ubuntu-latest",
  needs: [buildJob],
  if: isTag,
  steps: [
    step({
      name: "Download artifacts",
      uses: "actions/download-artifact@v6",
    }),
    step({
      name: "Output checksums",
      run: profiles.map(profile => {
        const output = [
          `echo "${profile.zipFileName}: ${buildJob.outputs[profile.zipChecksumEnvVarName]}"`,
        ];
        if (profile.target === "x86_64-pc-windows-msvc") {
          output.push(`echo "${profile.installerFileName}: ${buildJob.outputs[profile.installerChecksumEnvVarName]}"`);
        }
        return output;
      }).flat(),
    }),
    step({
      name: "Create SHASUMS256.txt file",
      run: profiles.map((profile, i) => {
        const op = i === 0 ? ">" : ">>";
        const output = [
          `echo "${buildJob.outputs[profile.zipChecksumEnvVarName]} ${profile.zipFileName}" ${op} SHASUMS256.txt`,
        ];
        if (profile.target === "x86_64-pc-windows-msvc") {
          output.push(`echo "${buildJob.outputs[profile.installerChecksumEnvVarName]} ${profile.installerFileName}" >> SHASUMS256.txt`);
        }
        return output;
      }).flat(),
    }),
    step({
      name: "Draft release",
      uses: "softprops/action-gh-release@v2",
      env: {
        GITHUB_TOKEN: expr("secrets.GITHUB_TOKEN"),
      },
      with: {
        files: [
          ...profiles.map(profile => {
            const output = [
              `${profile.artifactsName}/${profile.zipFileName}`,
            ];
            if (profile.target === "x86_64-pc-windows-msvc") {
              output.push(
                `${profile.artifactsName}/${profile.installerFileName}`,
              );
            }
            return output;
          }).flat(),
          "SHASUMS256.txt",
        ].join("\n"),
        body: `## Changes

* TODO

## Install

Run \`dprint upgrade\` or see https://dprint.dev/install/

## Checksums

|Artifact|SHA-256 Checksum|
|:--|:--|
${
          profiles.map(profile => {
            const output: [string, string][] = [
              [profile.zipFileName, `${buildJob.outputs[profile.zipChecksumEnvVarName]}`],
            ];
            if (profile.target === "x86_64-pc-windows-msvc") {
              output.push(
                [profile.installerFileName, `${buildJob.outputs[profile.installerChecksumEnvVarName]}`],
              );
            }
            return output.map(([name, checksum]) => `|${name}|${checksum}|`);
          }).flat().join("\n")
        }
`,
        draft: true,
      },
    }),
  ],
});

// === generate ===

workflow({
  name: "CI",
  on: {
    pull_request: { branches: ["main"] },
    push: { branches: ["main"], tags: ["*"] },
  },
  concurrency: {
    // https://stackoverflow.com/a/72408109/188246
    group: "${{ github.workflow }}-${{ github.head_ref || github.run_id }}",
    cancelInProgress: true,
  },
  jobs: [
    buildJob,
    draftReleaseJob,
  ],
}).writeOrLint({
  filePath: new URL("./ci.generated.yml", import.meta.url),
  header: "# GENERATED BY ./ci.ts -- DO NOT DIRECTLY EDIT",
});

if (!isLinting) {
  await $`dprint fmt --log-level=warn "**/*.yml"`;
}
