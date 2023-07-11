use anyhow::bail;
use anyhow::Result;
use std::path::PathBuf;

use crate::environment;

pub fn which_global(command_name: &str, environment: &impl environment::Environment) -> Result<PathBuf> {
  let mut search_dirs = vec![];
  if let Some(path) = environment.var("PATH") {
    for folder in path.split(if cfg!(windows) { ';' } else { ':' }) {
      search_dirs.push(PathBuf::from(folder));
    }
  }
  let path_exts = if cfg!(windows) {
    let uc_command_name = command_name.to_uppercase();
    let path_ext = environment.var("PATHEXT").unwrap_or_else(|| ".EXE;.CMD;.BAT;.COM".to_string());
    let command_exts = path_ext
      .split(';')
      .map(|s| s.trim().to_uppercase())
      .filter(|s| !s.is_empty())
      .collect::<Vec<_>>();
    if command_exts.is_empty() || command_exts.iter().any(|ext| uc_command_name.ends_with(ext)) {
      None // use the command name as-is
    } else {
      Some(command_exts)
    }
  } else {
    None
  };

  for search_dir in search_dirs {
    let paths = if let Some(path_exts) = &path_exts {
      let mut paths = Vec::new();
      for path_ext in path_exts {
        paths.push(search_dir.join(format!("{command_name}{path_ext}")))
      }
      paths
    } else {
      vec![search_dir.join(command_name)]
    };
    for path in paths {
      if environment.path_is_file(&path) {
        return Ok(path);
      }
    }
  }

  bail!("{}: command not found", command_name)
}
