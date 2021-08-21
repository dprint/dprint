use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::environment::Environment;
use crate::utils::get_bytes_hash;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IncrementalFileData {
  plugins_hash: u64,
  file_hashes: HashMap<PathBuf, u64>,
}

impl IncrementalFileData {
  pub fn new(plugins_hash: u64) -> IncrementalFileData {
    IncrementalFileData {
      plugins_hash,
      file_hashes: HashMap::new(),
    }
  }
}

pub struct IncrementalFile<TEnvironment: Environment> {
  file_path: PathBuf,
  read_data: IncrementalFileData,
  write_data: Mutex<IncrementalFileData>,
  base_dir_path: PathBuf,
  environment: TEnvironment,
}

impl<TEnvironment: Environment> IncrementalFile<TEnvironment> {
  pub fn new(file_path: PathBuf, plugins_hash: u64, environment: TEnvironment, base_dir_path: PathBuf) -> Self {
    let read_data = read_incremental(&file_path, &environment);
    let read_data = if let Some(read_data) = read_data {
      if read_data.plugins_hash == plugins_hash {
        read_data
      } else {
        log_verbose!(environment, "Plugins changed. Creating new incremental file.");
        IncrementalFileData::new(plugins_hash)
      }
    } else {
      IncrementalFileData::new(plugins_hash)
    };
    IncrementalFile {
      file_path,
      read_data,
      write_data: Mutex::new(IncrementalFileData::new(plugins_hash)),
      base_dir_path,
      environment,
    }
  }

  pub fn is_file_same(&self, file_path: &Path, file_text: &str) -> bool {
    let file_path = self.standardize_path(file_path);
    if let Some(hash) = self.read_data.file_hashes.get(&file_path) {
      if *hash == get_bytes_hash(file_text.as_bytes()) {
        // the file is the same, so save it in the write data
        self.add_to_write_data(file_path, file_text);
        true
      } else {
        false
      }
    } else {
      false
    }
  }

  pub fn update_file(&self, file_path: &Path, file_text: &str) {
    self.add_to_write_data(self.standardize_path(file_path), file_text)
  }

  fn add_to_write_data(&self, file_path: PathBuf, file_text: &str) {
    let hash = get_bytes_hash(file_text.as_bytes());
    let mut write_data = self.write_data.lock();
    write_data.file_hashes.insert(file_path, hash);
  }

  pub fn write(&self) {
    let write_data = self.write_data.lock();
    write_incremental(&self.file_path, &write_data, &self.environment);
  }

  fn standardize_path(&self, file_path: &Path) -> PathBuf {
    // need to ensure the file is stored as an absolute path
    if self.environment.is_absolute_path(file_path) {
      file_path.to_owned()
    } else {
      self.base_dir_path.join(file_path)
    }
  }
}

fn read_incremental(file_path: &Path, environment: &impl Environment) -> Option<IncrementalFileData> {
  let file_text = match environment.read_file(file_path) {
    Ok(file_text) => file_text,
    Err(err) => {
      if environment.path_exists(file_path) {
        environment.log_error(&format!("Error reading incremental file {}: {}", file_path.display(), err.to_string()));
      }
      return None;
    }
  };

  match serde_json::from_str(&file_text) {
    Ok(file_data) => Some(file_data),
    Err(err) => {
      environment.log_error(&format!("Error deserializing incremental file {}: {}", file_path.display(), err.to_string()));
      None
    }
  }
}

fn write_incremental(file_path: &Path, file_data: &IncrementalFileData, environment: &impl Environment) {
  let json_text = match serde_json::to_string(&file_data) {
    Ok(json_text) => json_text,
    Err(err) => {
      environment.log_error(&format!("Error serializing incremental file {}: {}", file_path.display(), err.to_string()));
      return;
    }
  };
  match environment.write_file(file_path, &json_text) {
    Err(err) => {
      environment.log_error(&format!("Error saving incremental file {}: {}", file_path.display(), err.to_string()));
    }
    _ => {}
  };
}
