//! Finds the dprint executable that's installed in a project's `node_modules`
//! folder, which an editor uses instead of the one it started with so that
//! the project is formatted with the version of dprint it specifies.

use std::path::Path;
use std::path::PathBuf;

use crate::environment::Environment;

use super::messages::LocalExecutable;

/// Finds the dprint executable installed with npm in the directory or one of
/// its ancestors (ex. `node_modules/@dprint/linux-x64-glibc/dprint`).
pub fn find_local_executable(environment: &impl Environment, dir_path: &Path) -> Option<LocalExecutable> {
  let package_names = get_platform_package_names();
  for dir_path in dir_path.ancestors() {
    let node_modules_dir = dir_path.join("node_modules");
    if !environment.path_exists(&node_modules_dir) {
      continue;
    }
    for package_name in &package_names {
      // hoisted to the top level node_modules folder
      if let Some(executable) = get_package_executable(environment, &node_modules_dir.join("@dprint").join(package_name)) {
        return Some(executable);
      }
      // A package manager may not hoist the platform package, so it's found
      // relative to the real location of the `dprint` package (ex. pnpm puts it
      // beside the `dprint` package in `node_modules/.pnpm/dprint@<version>/node_modules`).
      if let Ok(dprint_package_dir) = environment.canonicalize(node_modules_dir.join("dprint")) {
        let dprint_package_dir = dprint_package_dir.into_path_buf();
        let candidates = [
          dprint_package_dir.join("node_modules").join("@dprint").join(package_name),
          dprint_package_dir
            .parent()
            .map(|dir| dir.join("@dprint").join(package_name))
            .unwrap_or_default(),
        ];
        for candidate in candidates {
          if let Some(executable) = get_package_executable(environment, &candidate) {
            return Some(executable);
          }
        }
      }
    }
  }
  None
}

/// Gets the executable of the platform package in the directory.
fn get_package_executable(environment: &impl Environment, package_dir: &Path) -> Option<LocalExecutable> {
  let executable_name = if cfg!(windows) { "dprint.exe" } else { "dprint" };
  let path = package_dir.join(executable_name);
  if !environment.path_is_file(&path) {
    return None;
  }
  #[derive(serde::Deserialize)]
  struct PackageJson {
    version: String,
  }
  let package_json = environment.read_file(package_dir.join("package.json")).ok()?;
  let package_json = serde_json::from_str::<PackageJson>(&package_json).ok()?;
  Some(LocalExecutable {
    path: normalize_path(path),
    version: package_json.version,
  })
}

/// Gets the names of the npm packages of dprint for this platform (ex.
/// `linux-x64-glibc`), which are in order of preference.
fn get_platform_package_names() -> Vec<String> {
  let os = match std::env::consts::OS {
    "windows" => "win32",
    "macos" => "darwin",
    os => os,
  };
  let cpu = match std::env::consts::ARCH {
    "x86_64" => "x64",
    "aarch64" => "arm64",
    "riscv64" => "riscv64",
    "loongarch64" => "loong64",
    "powerpc64" => "ppc64",
    arch => arch,
  };
  if os != "linux" {
    return vec![format!("{}-{}", os, cpu)];
  }
  // npm installs the package of the system's libc, which may not be the one
  // this executable was built for
  let libcs = if cfg!(target_env = "musl") { ["musl", "glibc"] } else { ["glibc", "musl"] };
  libcs.iter().map(|libc| format!("{}-{}-{}", os, cpu, libc)).collect()
}

/// Removes the `.` and `..` components of a path that was joined to.
fn normalize_path(path: PathBuf) -> PathBuf {
  let mut result = PathBuf::new();
  for component in path.components() {
    match component {
      std::path::Component::CurDir => {}
      std::path::Component::ParentDir => {
        result.pop();
      }
      component => result.push(component),
    }
  }
  result
}

#[cfg(test)]
mod test {
  use std::path::Path;

  use pretty_assertions::assert_eq;

  use super::*;
  use crate::environment::TestEnvironment;

  #[test]
  fn should_find_the_executable_in_node_modules() {
    let environment = TestEnvironment::new();
    let package_name = &get_platform_package_names()[0];
    let executable_name = if cfg!(windows) { "dprint.exe" } else { "dprint" };
    let package_dir = format!("/project/node_modules/@dprint/{}", package_name);
    environment.mk_dir_all(&package_dir).unwrap();
    environment.write_file(format!("{}/{}", package_dir, executable_name), "").unwrap();
    environment
      .write_file(format!("{}/package.json", package_dir), r#"{ "version": "0.65.0" }"#)
      .unwrap();
    environment.mk_dir_all("/project/sub").unwrap();

    let expected = LocalExecutable {
      path: PathBuf::from(format!("{}/{}", package_dir, executable_name)),
      version: "0.65.0".to_string(),
    };
    // found from a descendant directory
    assert_eq!(find_local_executable(&environment, Path::new("/project/sub")), Some(expected.clone()));
    assert_eq!(find_local_executable(&environment, Path::new("/project")), Some(expected));
    assert_eq!(find_local_executable(&environment, Path::new("/other")), None);
  }
}
