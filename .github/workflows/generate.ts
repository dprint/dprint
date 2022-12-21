import * as yaml from "https://deno.land/std@0.170.0/encoding/yaml.ts";

enum OperatingSystem {
  Mac = "macOS-latest",
  Windows = "windows-latest",
  Linux = "ubuntu-18.04",
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
    group: "${{ github.workflow }}-${{ github.event.pull_request.number || github.ref }}",
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
        { uses: "dtolnay/rust-toolchain@stable" },
        { uses: "Swatinem/rust-cache@v2" },
        {
          name: "Build test plugins",
          if: "matrix.config.run_tests == 'true'",
          run: [
            "rustup target add wasm32-unknown-unknown",
            "cargo build --manifest-path=crates/test-plugin/Cargo.toml --release --target=wasm32-unknown-unknown",
            "cargo build --manifest-path=crates/test-process-plugin/Cargo.toml --release --locked",
          ].join("\n"),
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
            "sudo apt install -y gcc-aarch64-linux-gnu",
            "rustup target add aarch64-unknown-linux-gnu",
            "export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc",
          ].join("\n"),
        },
        {
          name: "Setup (Mac aarch64)",
          if: "matrix.config.target == 'aarch64-apple-darwin'",
          run: "rustup target add aarch64-apple-darwin",
        },
        {
          name: "Build release",
          run: "cargo build -p dprint --locked --all-features --release --target ${{matrix.config.target}}",
        },
        {
          name: "Test",
          if: "matrix.config.run_tests == 'true'",
          run: "cargo test --locked --all-features --release",
        },
        {
          name: "Create installer (Windows x86_64)",
          uses: "joncloud/makensis-action@v2.0",
          if: "startsWith(matrix.config.os, 'windows')",
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
            if: `matrix.config.target == '${profile.target}'`,
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
            if: `matrix.config.target == '${profile.target}'`,
            uses: "actions/upload-artifact@v2",
            with: {
              name: profile.artifactsName,
              path: getArtifactPaths().join("\n"),
            },
          };
        }),
        {
          name: "Test shell installer",
          run: [
            "cd website/src/assets",
            "chmod +x install.sh",
            "./install.sh",
          ].join("\n"),
        },
        {
          name: "Test powershell installer (Windows)",
          if: "startsWith(matrix.config.os, 'windows')",
          shell: "pwsh",
          run: ["cd website/src/assets", "./install.ps1"].join("\n"),
        },
        {
          name: "Test npm",
          run: [
            "cd deployment/npm",
            "curl --fail --location --progress-bar --output \"SHASUMS256.txt\" \"https://github.com/dprint/dprint/releases/download/0.34.1/SHASUMS256.txt\"",
            "node setup.js 0.34.1",
            "npm install",
            "node bin.js -v",
          ].join("\n"),
        },
      ],
    },
    draft_release: {
      name: "draft_release",
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
          run: profiles.map(profile => {
            const output = [
              `echo "\${{needs.build.outputs.${profile.zipChecksumEnvVarName}}} ${profile.zipFileName}" > SHASUMS256.txt`,
            ];
            if (profile.os === OperatingSystem.Windows) {
              output.push(`echo "\${{needs.build.outputs.${profile.installerChecksumEnvVarName}}} ${profile.installerFileName}" > SHASUMS256.txt`);
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
            files: profiles.map(profile => {
              const output = [
                `${profile.artifactsName}/${profile.zipFileName}`,
              ];
              if (profile.os === OperatingSystem.Windows) {
                output.push(
                  `${profile.artifactsName}/${profile.installerFileName}`,
                );
              }
              return output;
            }).flat().join("\n"),
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

let finalText = `# AUTO-GENERATED BY ./generate.ts -- DO NOT DIRECTLY EDIT.\n\n`;
finalText += yaml.stringify(ci, {
  noRefs: true,
  lineWidth: 10_000,
  noCompatMode: true,
});

Deno.writeTextFileSync(new URL("./ci.yml", import.meta.url), finalText);
