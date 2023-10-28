import * as yaml from "https://deno.land/std@0.170.0/encoding/yaml.ts";
import $ from "https://deno.land/x/dax@0.33.0/mod.ts";

enum OperatingSystem {
  Mac = "macOS-latest",
  Windows = "windows-latest",
  Linux = "ubuntu-20.04",
}

interface ProfileData {
  os: OperatingSystem;
  target: string;
  runTests?: boolean;
}

const profileDataItems: ProfileData[] = [{
  os: OperatingSystem.Mac,
  target: "x86_64-apple-darwin",
  runTests: true,
}, {
  os: OperatingSystem.Mac,
  target: "aarch64-apple-darwin",
}, {
  os: OperatingSystem.Windows,
  target: "x86_64-pc-windows-msvc",
  runTests: true,
}, {
  os: OperatingSystem.Linux,
  target: "x86_64-unknown-linux-gnu",
  runTests: true,
}, {
  os: OperatingSystem.Linux,
  target: "x86_64-unknown-linux-musl",
}, {
  os: OperatingSystem.Linux,
  target: "aarch64-unknown-linux-gnu",
}, {
  os: OperatingSystem.Linux,
  target: "aarch64-unknown-linux-musl",
}];
const profiles = profileDataItems.map(profile => {
  return {
    ...profile,
    zipChecksumEnvVarName: `ZIP_CHECKSUM_${profile.target.toUpperCase().replaceAll("-", "_")}`,
    get installerChecksumEnvVarName() {
      if (profile.os !== OperatingSystem.Windows) {
        throw new Error("Check for windows before accessing.");
      }
      return `INSTALLER_CHECKSUM_${profile.target.toUpperCase().replaceAll("-", "_")}`;
    },
    artifactsName: `${profile.target}-artifacts`,
    zipFileName: `dprint-${profile.target}.zip`,
    get installerFileName() {
      if (profile.os !== OperatingSystem.Windows) {
        throw new Error("Check for windows before accessing.");
      }
      return `dprint-${profile.target}-installer.exe`;
    },
  };
});

const ci = {
  name: "CI",
  on: {
    pull_request: { branches: ["main"] },
    push: { branches: ["main"], tags: ["*"] },
  },
  concurrency: {
    // https://stackoverflow.com/a/72408109/188246
    group: "${{ github.workflow }}-${{ github.head_ref || github.run_id }}",
    "cancel-in-progress": true,
  },
  jobs: {
    build: {
      name: "${{ matrix.config.target }}",
      "runs-on": "${{ matrix.config.os }}",
      strategy: {
        matrix: {
          config: profiles.map(profile => ({
            os: profile.os,
            run_tests: (profile.runTests ?? false).toString(),
            target: profile.target,
          })),
        },
      },
      env: {
        // disabled to reduce ./target size and generally it's slower enabled
        CARGO_INCREMENTAL: 0,
        RUST_BACKTRACE: "full",
      },
      outputs: Object.fromEntries(
        profiles.map(profile => {
          const entries = [];
          entries.push([
            profile.zipChecksumEnvVarName,
            "${{steps.pre_release_" + profile.target.replaceAll("-", "_") + ".outputs.ZIP_CHECKSUM}}",
          ]);
          if (profile.os === OperatingSystem.Windows) {
            entries.push([
              profile.installerChecksumEnvVarName,
              "${{steps.pre_release_" + profile.target.replaceAll("-", "_") + ".outputs.INSTALLER_CHECKSUM}}",
            ]);
          }
          return entries;
        }).flat(),
      ),
      steps: [
        { name: "Checkout", uses: "actions/checkout@v2" },
        { uses: "dsherret/rust-toolchain-file@v1" },
        { uses: "Swatinem/rust-cache@v2" },
        { uses: "denoland/setup-deno@v1" },
        {
          name: "Verify wasmer-compiler version",
          if: "matrix.config.target == 'x86_64-unknown-linux-gnu'",
          run: "deno run --allow-env --allow-read --allow-net=deno.land .github/workflows/scripts/verify_wasmer_compiler_version.ts",
        },
        {
          name: "Build test plugins (Debug)",
          if: "matrix.config.run_tests == 'true' && !startsWith(github.ref, 'refs/tags/')",
          run: "cargo build -p test-process-plugin --locked --target ${{matrix.config.target}}",
        },
        {
          name: "Build test plugins (Release)",
          if: "matrix.config.run_tests == 'true' && startsWith(github.ref, 'refs/tags/')",
          run: "cargo build -p test-process-plugin --locked --target ${{matrix.config.target}} --release",
        },
        {
          name: "Setup (Linux x86_64-musl)",
          if: "matrix.config.target == 'x86_64-unknown-linux-musl'",
          run: [
            "sudo apt update",
            "sudo apt install musl musl-dev musl-tools",
            "rustup target add x86_64-unknown-linux-musl",
          ].join("\n"),
        },
        {
          name: "Setup (Linux aarch64)",
          if: "matrix.config.target == 'aarch64-unknown-linux-gnu'",
          run: [
            "sudo apt update",
            "sudo apt install gcc-aarch64-linux-gnu",
            "rustup target add aarch64-unknown-linux-gnu",
          ].join("\n"),
        },
        {
          name: "Setup (Linux aarch64-musl)",
          if: "matrix.config.target == 'aarch64-unknown-linux-musl'",
          run: [
            "sudo apt update",
            "sudo apt install gcc-aarch64-linux-gnu",
            "sudo apt install musl musl-dev musl-tools",
            "rustup target add aarch64-unknown-linux-musl",
          ].join("\n"),
        },
        {
          name: "Setup (Mac aarch64)",
          if: "matrix.config.target == 'aarch64-apple-darwin'",
          run: "rustup target add aarch64-apple-darwin",
        },
        {
          name: "Build (Debug)",
          if: "!startsWith(github.ref, 'refs/tags/')",
          env: {
            "CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER": "aarch64-linux-gnu-gcc",
            "CC_aarch64_unknown_linux_musl": "aarch64-linux-gnu-gcc",
          },
          run: [
            "cargo build -p dprint --locked --target ${{matrix.config.target}}",
          ].join("\n"),
        },
        {
          name: "Build (Release)",
          if: "startsWith(github.ref, 'refs/tags/')",
          env: {
            "CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER": "aarch64-linux-gnu-gcc",
            "CC_aarch64_unknown_linux_musl": "aarch64-linux-gnu-gcc",
          },
          run: [
            "cargo build -p dprint --locked --target ${{matrix.config.target}} --release",
          ].join("\n"),
        },
        {
          name: "Test (Debug)",
          if: "matrix.config.run_tests == 'true' && !startsWith(github.ref, 'refs/tags/')",
          run: "cargo test --locked --target ${{matrix.config.target}} --all-features",
        },
        {
          name: "Test (Release)",
          if: "matrix.config.run_tests == 'true' && startsWith(github.ref, 'refs/tags/')",
          run: "cargo test --locked --target ${{matrix.config.target}} --all-features --release",
        },
        {
          name: "Test integration",
          if: "matrix.config.target == 'x86_64-unknown-linux-gnu' && !startsWith(github.ref, 'refs/tags/')",
          run: "cargo run -p dprint --locked --target ${{matrix.config.target}} -- check",
        },
        {
          name: "Create installer (Windows x86_64)",
          uses: "joncloud/makensis-action@v2.0",
          if: "startsWith(matrix.config.os, 'windows') && startsWith(github.ref, 'refs/tags/')",
          with: { "script-file": "${{ github.workspace }}/deployment/installer/dprint-installer.nsi" },
        },
        // zip files
        ...profiles.map(profile => {
          function getRunSteps() {
            switch (profile.os) {
              case OperatingSystem.Mac:
                return [
                  `cd target/${profile.target}/release`,
                  `zip -r ${profile.zipFileName} dprint`,
                  `echo \"::set-output name=ZIP_CHECKSUM::$(shasum -a 256 ${profile.zipFileName} | awk '{print $1}')\"`,
                ];
              case OperatingSystem.Linux:
                return [
                  `cd target/${profile.target}/release`,
                  `zip -r ${profile.zipFileName} dprint`,
                  `echo \"::set-output name=ZIP_CHECKSUM::$(shasum -a 256 ${profile.zipFileName} | awk '{print $1}')\"`,
                ];
              case OperatingSystem.Windows:
                return [
                  `Compress-Archive -CompressionLevel Optimal -Force -Path target/${profile.target}/release/dprint.exe -DestinationPath target/${profile.target}/release/${profile.zipFileName}`,
                  `mv deployment/installer/${profile.installerFileName} target/${profile.target}/release/${profile.installerFileName}`,
                  `echo "::set-output name=ZIP_CHECKSUM::$(shasum -a 256 target/${profile.target}/release/${profile.zipFileName} | awk '{print $1}')"`,
                  `echo "::set-output name=INSTALLER_CHECKSUM::$(shasum -a 256 target/${profile.target}/release/${profile.installerFileName} | awk '{print $1}')"`,
                ];
            }
          }
          return {
            name: `Pre-release (${profile.target})`,
            id: `pre_release_${profile.target.replaceAll("-", "_")}`,
            if: `matrix.config.target == '${profile.target}' && startsWith(github.ref, 'refs/tags/')`,
            run: getRunSteps().join("\n"),
          };
        }),
        // upload artifacts
        ...profiles.map(profile => {
          function getArtifactPaths() {
            const paths = [
              `target/${profile.target}/release/${profile.zipFileName}`,
            ];
            if (profile.os === OperatingSystem.Windows) {
              paths.push(
                `target/${profile.target}/release/${profile.installerFileName}`,
              );
            }
            return paths;
          }

          return {
            name: `Upload artifacts (${profile.target})`,
            if: `matrix.config.target == '${profile.target}' && startsWith(github.ref, 'refs/tags/')`,
            uses: "actions/upload-artifact@v2",
            with: {
              name: profile.artifactsName,
              path: getArtifactPaths().join("\n"),
            },
          };
        }),
        {
          name: "Test shell installer",
          if: "matrix.config.run_tests == 'true' && !startsWith(github.ref, 'refs/tags/')",
          run: [
            "cd website/src/assets",
            "chmod +x install.sh",
            "./install.sh",
          ].join("\n"),
        },
        {
          name: "Test powershell installer (Windows)",
          if: "matrix.config.run_tests == 'true' && !startsWith(github.ref, 'refs/tags/') && startsWith(matrix.config.os, 'windows')",
          shell: "pwsh",
          run: ["cd website/src/assets", "./install.ps1"].join("\n"),
        },
        // todo: temporarily ignore for aarch64-musl because a release hasn't been done with this
        // {
        //   name: "Test npm",
        //   if: "matrix.config.run_tests == 'true' && !startsWith(github.ref, 'refs/tags/')",
        //   run: [
        //     "cd deployment/npm",
        //     "deno run -A build.ts 0.37.1",
        //   ].join("\n"),
        // },
      ],
    },
    draft_release: {
      name: "draft_release",
      if: "startsWith(github.ref, 'refs/tags/')",
      needs: "build",
      "runs-on": "ubuntu-latest",
      steps: [
        {
          name: "Download artifacts",
          uses: "actions/download-artifact@v2",
        },
        {
          name: "Output checksums",
          run: profiles.map(profile => {
            const output = [
              `echo "${profile.zipFileName}: \${{needs.build.outputs.${profile.zipChecksumEnvVarName}}}"`,
            ];
            if (profile.os === OperatingSystem.Windows) {
              output.push(`echo "${profile.installerFileName}: \${{needs.build.outputs.${profile.installerChecksumEnvVarName}}}"`);
            }
            return output;
          }).flat().join("\n"),
        },
        {
          name: "Create SHASUMS256.txt file",
          run: profiles.map((profile, i) => {
            const op = i === 0 ? ">" : ">>";
            const output = [
              `echo "\${{needs.build.outputs.${profile.zipChecksumEnvVarName}}} ${profile.zipFileName}" ${op} SHASUMS256.txt`,
            ];
            if (profile.os === OperatingSystem.Windows) {
              output.push(`echo "\${{needs.build.outputs.${profile.installerChecksumEnvVarName}}} ${profile.installerFileName}" >> SHASUMS256.txt`);
            }
            return output;
          }).flat().join("\n"),
        },
        {
          name: "Draft release",
          uses: "softprops/action-gh-release@v1",
          env: {
            GITHUB_TOKEN: "${{ secrets.GITHUB_TOKEN }}",
          },
          with: {
            files: [
              ...profiles.map(profile => {
                const output = [
                  `${profile.artifactsName}/${profile.zipFileName}`,
                ];
                if (profile.os === OperatingSystem.Windows) {
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
                const output = [
                  [`${profile.zipFileName}`, profile.zipChecksumEnvVarName],
                ];
                if (profile.os === OperatingSystem.Windows) {
                  output.push(
                    [`${profile.installerFileName}`, profile.installerChecksumEnvVarName],
                  );
                }
                return output.map(([name, envVar]) => `|${name}|\${{needs.build.outputs.${envVar}}}|`);
              }).flat().join("\n")
            }
`,
            draft: true,
          },
        },
      ],
    },
  },
};

let finalText = `# GENERATED BY ./ci.generate.ts -- DO NOT DIRECTLY EDIT\n\n`;
finalText += yaml.stringify(ci, {
  noRefs: true,
  lineWidth: 10_000,
  noCompatMode: true,
});

Deno.writeTextFileSync(new URL("./ci.yml", import.meta.url), finalText);

await $`dprint fmt --log-level=warn "**/*.yml"`;
