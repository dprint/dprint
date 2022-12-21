import * as yaml from "https://deno.land/std@0.170.0/encoding/yaml.ts";

enum RunnerOperatingSystem {
  Mac = "macOS-latest",
  Windows = "windows-latest",
  Ubuntu = "ubuntu-18.0.3",
}

interface Profile {
  operatingSystem: RunnerOperatingSystem;
}

const profiles: Profile[] = [{
  operatingSystem: RunnerOperatingSystem.Mac,
}];

const ci = {
  name: "CI",
  on: {
    pull_request: { branches: ["main"] },
    push: { branches: ["main"], tags: ["*"] },
  },
  jobs: {
    build: {
      name: "${{ matrix.config.kind }} ${{ matrix.config.os }}",
      "runs-on": "${{ matrix.config.os }}",
      strategy: {
        matrix: {
          config: [
            { os: "macOS-latest", kind: "test_release" },
            { os: "windows-latest", kind: "test_release" },
            // uses an older version of ubuntu because of issue #483
            { os: "ubuntu-18.04", kind: "test_release" },
            { os: "ubuntu-latest", kind: "test_debug" },
          ],
        },
      },
      env: {
        // disabled to reduce ./target size and generally it's slower enabled
        CARGO_INCREMENTAL: 0,
        RUST_BACKTRACE: "full",
      },
      outputs: {
        LINUX_X86_64_ZIP_CHECKSUM: "${{steps.linux_x86_64_pre_release.outputs.ZIP_CHECKSUM}}",
        LINUX_X86_64_MUSL_ZIP_CHECKSUM: "${{steps.linux_x86_64_musl_pre_release.outputs.ZIP_CHECKSUM}}",
        LINUX_AARCH64_ZIP_CHECKSUM: "${{steps.linux_aarch64_pre_release.outputs.ZIP_CHECKSUM}}",
        MAX_X86_64_ZIP_CHECKSUM: "${{steps.mac_x86_64_pre_release.outputs.ZIP_CHECKSUM}}",
        MAX_AARCH64_ZIP_CHECKSUM: "${{steps.mac_aarch64_pre_release.outputs.ZIP_CHECKSUM}}",
        WINDOWS_X86_64_ZIP_CHECKSUM: "${{steps.windows_x86_64_pre_release.outputs.ZIP_CHECKSUM}}",
        WINDOWS_INSTALLER_CHECKSUM: "${{steps.windows_x86_64_pre_release.outputs.INSTALLER_CHECKSUM}}",
      },
      steps: [
        { name: "Checkout", uses: "actions/checkout@v2" },
        { uses: "dtolnay/rust-toolchain@stable" },
        { name: "Install wasm32 target", run: "rustup target add wasm32-unknown-unknown" },
        // todo: re-enable this for ubuntu... was having cache issues with glib
        { uses: "Swatinem/rust-cache@v1", if: "startsWith(matrix.config.os, 'ubuntu') == false" },
        {
          name: "Build test plugins",
          run: [
            "cargo build --manifest-path=crates/test-plugin/Cargo.toml --release --target=wasm32-unknown-unknown",
            "cargo build --manifest-path=crates/test-process-plugin/Cargo.toml --release --locked",
          ].join("\n"),
        },
        { name: "Build debug", if: "matrix.config.kind == 'test_debug'", run: "cargo build --locked --all-features" },
        { name: "Build release", if: "matrix.config.kind == 'test_release'", run: "cargo build --locked --all-features --release" },
        {
          name: "Build release (Linux x86_64-musl)",
          if: "startsWith(matrix.config.os, 'ubuntu') && matrix.config.kind == 'test_release'",
          run: [
            "sudo apt update",
            "sudo apt install musl musl-dev musl-tools",
            "rustup target add x86_64-unknown-linux-musl",
            "cargo build -p dprint --locked --all-features --release --target x86_64-unknown-linux-musl",
          ].join("\n"),
        },
        {
          name: "Build release (Linux aarch64)",
          if: "startsWith(matrix.config.os, 'ubuntu') && matrix.config.kind == 'test_release'",
          run: [
            "rustup target add aarch64-unknown-linux-gnu",
            "sudo apt install -y gcc-aarch64-linux-gnu",
            "export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc",
            "cargo build -p dprint --locked --all-features --release --target aarch64-unknown-linux-gnu",
          ].join("\n"),
        },
        {
          name: "Build release (Mac aarch64)",
          if: "startsWith(matrix.config.os, 'macOS') && matrix.config.kind == 'test_release'",
          run: [
            "rustup target add aarch64-apple-darwin",
            "cargo build -p dprint --locked --all-features --release --target aarch64-apple-darwin",
          ].join("\n"),
        },
        { name: "Test debug", if: "matrix.config.kind == 'test_debug'", run: "cargo test --locked --all-features" },
        { name: "Test release", if: "matrix.config.kind == 'test_release'", run: "cargo test --locked --all-features --release" },
        {
          name: "Create installer (Windows x86_64)",
          uses: "joncloud/makensis-action@v2.0",
          if: "startsWith(matrix.config.os, 'windows') && matrix.config.kind == 'test_release' && startsWith(github.ref, 'refs/tags/')",
          with: { "script-file": "${{ github.workspace }}/deployment/installer/dprint-installer.nsi" },
        },
        {
          name: "Pre-release (Linux x86_64)",
          id: "linux_x86_64_pre_release",
          if: "startsWith(matrix.config.os, 'ubuntu') && matrix.config.kind == 'test_release' && startsWith(github.ref, 'refs/tags/')",
          run: [
            "cd target/release",
            "zip -r dprint-x86_64-unknown-linux-gnu.zip dprint",
            "echo \"::set-output name=ZIP_CHECKSUM::$(shasum -a 256 dprint-x86_64-unknown-linux-gnu.zip | awk '{print $1}')\"",
          ].join("\n"),
        },
        {
          name: "Pre-release (Linux x86_64-musl)",
          id: "linux_x86_64_musl_pre_release",
          if: "startsWith(matrix.config.os, 'ubuntu') && matrix.config.kind == 'test_release' && startsWith(github.ref, 'refs/tags/')",
          run: [
            "cd target/x86_64-unknown-linux-musl/release",
            "zip -r dprint-x86_64-unknown-linux-musl.zip dprint",
            "echo \"::set-output name=ZIP_CHECKSUM::$(shasum -a 256 dprint-x86_64-unknown-linux-musl.zip | awk '{print $1}')\"",
            "mv dprint-x86_64-unknown-linux-musl.zip ../../release",
          ].join("\n"),
        },
        {
          name: "Pre-release (Linux aarch64)",
          id: "linux_aarch64_pre_release",
          if: "startsWith(matrix.config.os, 'ubuntu') && matrix.config.kind == 'test_release' && startsWith(github.ref, 'refs/tags/')",
          run: [
            "cd target/aarch64-unknown-linux-gnu/release",
            "zip -r dprint-aarch64-unknown-linux-gnu.zip dprint",
            "echo \"::set-output name=ZIP_CHECKSUM::$(shasum -a 256 dprint-aarch64-unknown-linux-gnu.zip | awk '{print $1}')\"",
            "mv dprint-aarch64-unknown-linux-gnu.zip ../../release",
          ].join("\n"),
        },
        {
          name: "Pre-release (Mac x86_64)",
          id: "mac_x86_64_pre_release",
          if: "startsWith(matrix.config.os, 'macOS') && matrix.config.kind == 'test_release' && startsWith(github.ref, 'refs/tags/')",
          run: [
            "cd target/release",
            "zip -r dprint-x86_64-apple-darwin.zip dprint",
            "echo \"::set-output name=ZIP_CHECKSUM::$(shasum -a 256 dprint-x86_64-apple-darwin.zip | awk '{print $1}')\"",
          ].join("\n"),
        },
        {
          name: "Pre-release (Mac aarch64)",
          id: "mac_aarch64_pre_release",
          if: "startsWith(matrix.config.os, 'macOS') && matrix.config.kind == 'test_release' && startsWith(github.ref, 'refs/tags/')",
          run: [
            "cd target/aarch64-apple-darwin/release",
            "zip -r dprint-aarch64-apple-darwin.zip dprint",
            "echo \"::set-output name=ZIP_CHECKSUM::$(shasum -a 256 dprint-aarch64-apple-darwin.zip | awk '{print $1}')\"",
            "mv dprint-aarch64-apple-darwin.zip ../../release",
          ].join("\n"),
        },
        {
          name: "Pre-release (Windows x86_64)",
          id: "windows_x86_64_pre_release",
          if: "startsWith(matrix.config.os, 'windows') && matrix.config.kind == 'test_release' && startsWith(github.ref, 'refs/tags/')",
          run: [
            "Compress-Archive -CompressionLevel Optimal -Force -Path target/release/dprint.exe -DestinationPath target/release/dprint-x86_64-pc-windows-msvc.zip",
            "mv deployment/installer/dprint-x86_64-pc-windows-msvc-installer.exe target/release/dprint-x86_64-pc-windows-msvc-installer.exe",
            "echo \"::set-output name=ZIP_CHECKSUM::$(shasum -a 256 target/release/dprint-x86_64-pc-windows-msvc.zip | awk '{print $1}')\"",
            "echo \"::set-output name=INSTALLER_CHECKSUM::$(shasum -a 256 target/release/dprint-x86_64-pc-windows-msvc-installer.exe | awk '{print $1}')\"",
          ].join("\n"),
        },
        {
          name: "Upload Artifacts (Linux)",
          uses: "actions/upload-artifact@v2",
          if: "startsWith(matrix.config.os, 'ubuntu') && matrix.config.kind == 'test_release' && startsWith(github.ref, 'refs/tags/')",
          with: {
            name: "linux-artifacts",
            path: [
              "target/release/dprint-aarch64-unknown-linux-gnu.zip",
              "target/release/dprint-x86_64-unknown-linux-gnu.zip",
              "target/release/dprint-x86_64-unknown-linux-musl.zip",
            ].join("\n"),
          },
        },
        {
          name: "Upload Artifacts (Mac)",
          uses: "actions/upload-artifact@v2",
          if: "startsWith(matrix.config.os, 'macOS') && matrix.config.kind == 'test_release' && startsWith(github.ref, 'refs/tags/')",
          with: {
            name: "mac-artifacts",
            path: [
              "target/release/dprint-aarch64-apple-darwin.zip",
              "target/release/dprint-x86_64-apple-darwin.zip",
            ].join("\n"),
          },
        },
        {
          name: "Upload Artifacts (Windows)",
          uses: "actions/upload-artifact@v2",
          if: "startsWith(matrix.config.os, 'windows') && matrix.config.kind == 'test_release' && startsWith(github.ref, 'refs/tags/')",
          with: {
            name: "windows-artifacts",
            path: [
              "target/release/dprint-x86_64-pc-windows-msvc.zip",
              "target/release/dprint-x86_64-pc-windows-msvc-installer.exe",
            ].join("\n"),
          },
        },
        { name: "Test shell installer", run: ["cd website/src/assets", "chmod +x install.sh", "./install.sh"].join("\n") },
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
            "curl --fail --location --progress-bar --output \"SHASUMS256.txt\" \"https://github.com/dprint/dprint/releases/download/0.30.2/SHASUMS256.txt\"",
            "# temporary until a musl release is done",
            "echo \"837859756888e579189459fb309a5a20a3c19e870254184a43d23a9f2ce12748 dprint-x86_64-unknown-linux-musl.zip\" >> SHASUMS256.txt",
            "node setup.js 0.30.2",
            "npm install",
            "node bin.js -v",
          ].join("\n"),
        },
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
          name: "Get tag version",
          id: "get_tag_version",
          run: "echo ::set-output name=TAG_VERSION::${GITHUB_REF/refs\\/tags\\//}",
        },
        {
          name: "Output checksums",
          run: [
            "echo \"Linux x86_64 Zip: ${{needs.build.outputs.LINUX_X86_64_ZIP_CHECKSUM}}\"",
            "echo \"Linux x86_64-musl Zip: ${{needs.build.outputs.LINUX_X86_64_MUSL_ZIP_CHECKSUM}}\"",
            "echo \"Linux aarch64 Zip: ${{needs.build.outputs.LINUX_AARCH64_ZIP_CHECKSUM}}\"",
            "echo \"Mac x86_64 Zip: ${{needs.build.outputs.MAX_X86_64_ZIP_CHECKSUM}}\"",
            "echo \"Mac aarch64 Zip: ${{needs.build.outputs.MAX_AARCH64_ZIP_CHECKSUM}}\"",
            "echo \"Windows x86_64 Zip: ${{needs.build.outputs.WINDOWS_X86_64_ZIP_CHECKSUM}}\"",
            "echo \"Windows x86_64 Installer: ${{needs.build.outputs.WINDOWS_INSTALLER_CHECKSUM}}\"",
          ].join("\n"),
        },
        {
          name: "Create SHASUMS256.txt file",
          run: [
            "echo \"${{needs.build.outputs.WINDOWS_X86_64_ZIP_CHECKSUM}} dprint-x86_64-pc-windows-msvc.zip\" > SHASUMS256.txt",
            "echo \"${{needs.build.outputs.LINUX_X86_64_ZIP_CHECKSUM}} dprint-x86_64-unknown-linux-gnu.zip\" >> SHASUMS256.txt",
            "echo \"${{needs.build.outputs.LINUX_X86_64_MUSL_ZIP_CHECKSUM}} dprint-x86_64-unknown-linux-musl.zip\" >> SHASUMS256.txt",
            "echo \"${{needs.build.outputs.LINUX_AARCH64_ZIP_CHECKSUM}} dprint-aarch64-unknown-linux-gnu.zip\" >> SHASUMS256.txt",
            "echo \"${{needs.build.outputs.MAX_X86_64_ZIP_CHECKSUM}} dprint-x86_64-apple-darwin.zip\" >> SHASUMS256.txt",
            "echo \"${{needs.build.outputs.MAX_AARCH64_ZIP_CHECKSUM}} dprint-aarch64-apple-darwin.zip\" >> SHASUMS256.txt",
            "echo \"${{needs.build.outputs.WINDOWS_INSTALLER_CHECKSUM}} dprint-x86_64-pc-windows-msvc-installer.exe\" >> SHASUMS256.txt",
          ].join("\n"),
        },
        {
          name: "Draft release",
          uses: "softprops/action-gh-release@v1",
          env: {
            GITHUB_TOKEN: "${{ secrets.GITHUB_TOKEN }}",
          },
          with: {
            files: [
              "windows-artifacts/dprint-x86_64-pc-windows-msvc.zip",
              "windows-artifacts/dprint-x86_64-pc-windows-msvc-installer.exe",
              "linux-artifacts/dprint-x86_64-unknown-linux-gnu.zip",
              "linux-artifacts/dprint-x86_64-unknown-linux-musl.zip",
              "linux-artifacts/dprint-aarch64-unknown-linux-gnu.zip",
              "mac-artifacts/dprint-x86_64-apple-darwin.zip",
              "mac-artifacts/dprint-aarch64-apple-darwin.zip",
              "SHASUMS256.txt",
            ].join("\n"),
            body: `## Changes

* TODO

## Install

Run \`dprint upgrade\` or see https://dprint.dev/install/

## Checksums

|Artifact|SHA-256 Checksum|
|:--|:--|
|Linux x86_64 Zip|\${{needs.build.outputs.LINUX_X86_64_ZIP_CHECKSUM}}|
|Linux x86_64-musl Zip|\${{needs.build.outputs.LINUX_X86_64_MUSL_ZIP_CHECKSUM}}|
|Linux aarch64 Zip|\${{needs.build.outputs.LINUX_AARCH64_ZIP_CHECKSUM}}|
|Mac x86_64 Zip|\${{needs.build.outputs.MAX_X86_64_ZIP_CHECKSUM}}|
|Mac aarch64 Zip|\${{needs.build.outputs.MAX_AARCH64_ZIP_CHECKSUM}}|
|Windows x86_64 Zip|\${{needs.build.outputs.WINDOWS_X86_64_ZIP_CHECKSUM}}|
|Windows x86_64 Installer|\${{needs.build.outputs.WINDOWS_INSTALLER_CHECKSUM}}|
`,
            draft: true,
          },
        },
      ],
    },
  },
};

let finalText = `# THIS FILE IS AUTO-GENERATED. DO NOT EDIT.\n# This CI configuration is generated by ./generate.ts.\n\n`;
finalText += yaml.stringify(ci, {
  noRefs: true,
  lineWidth: 10_000,
  noCompatMode: true,
});

Deno.writeTextFileSync(new URL("./ci.yml", import.meta.url), finalText);
