use parking_lot::Mutex;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;

use crate::environment::CanonicalizedPathBuf;
use crate::environment::Environment;
use crate::utils::get_bytes_hash;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IncrementalFileData {
  plugins_hash: u64,
  file_hashes: HashSet<u64>,
}

impl IncrementalFileData {
  pub fn new(plugins_hash: u64) -> IncrementalFileData {
    IncrementalFileData {
      plugins_hash,
      file_hashes: Default::default(),
    }
  }
}

pub struct IncrementalFile<TEnvironment: Environment> {
  file_path: CanonicalizedPathBuf,
  read_data: IncrementalFileData,
  write_data: Mutex<IncrementalFileData>,
  environment: TEnvironment,
}

impl<TEnvironment: Environment> IncrementalFile<TEnvironment> {
  pub fn new(file_path: CanonicalizedPathBuf, plugins_hash: u64, environment: TEnvironment) -> Self {
    let read_data = read_incremental(&file_path, &environment);
    let read_data = if let Some(read_data) = read_data {
      if read_data.plugins_hash == plugins_hash {
        read_data
      } else {
        log_debug!(environment, "Plugins changed. Creating new incremental file.");
        IncrementalFileData::new(plugins_hash)
      }
    } else {
      IncrementalFileData::new(plugins_hash)
    };
    IncrementalFile {
      file_path,
      read_data,
      write_data: Mutex::new(IncrementalFileData::new(plugins_hash)),
      environment,
    }
  }

  /// If the file text is known to be formatted.
  pub fn is_file_known_formatted(&self, file_text: &str) -> bool {
    let hash = get_bytes_hash(file_text.as_bytes());
    if self.read_data.file_hashes.contains(&hash) {
      // the file is the same, so save it in the write data
      self.add_to_write_data(hash);
      true
    } else {
      false
    }
  }

  pub fn update_file(&self, file_text: &str) {
    let hash = get_bytes_hash(file_text.as_bytes());
    self.add_to_write_data(hash)
  }

  fn add_to_write_data(&self, hash: u64) {
    let mut write_data = self.write_data.lock();
    write_data.file_hashes.insert(hash);
  }

  pub fn write(&self) {
    let write_data = self.write_data.lock();
    write_incremental(&self.file_path, &write_data, &self.environment);
  }
}

fn read_incremental(file_path: impl AsRef<Path>, environment: &impl Environment) -> Option<IncrementalFileData> {
  let file_text = match environment.read_file(&file_path) {
    Ok(file_text) => file_text,
    Err(err) => {
      if environment.path_exists(&file_path) {
        log_warn!(environment, "Error reading incremental file {}: {}", file_path.as_ref().display(), err);
      }
      return None;
    }
  };

  match serde_json::from_str(&file_text) {
    Ok(file_data) => Some(file_data),
    Err(err) => {
      log_warn!(environment, "Error deserializing incremental file {}: {}", file_path.as_ref().display(), err);
      None
    }
  }
}

fn write_incremental(file_path: impl AsRef<Path>, file_data: &IncrementalFileData, environment: &impl Environment) {
  let json_text = match serde_json::to_string(&file_data) {
    Ok(json_text) => json_text,
    Err(err) => {
      log_warn!(environment, "Error serializing incremental file {}: {}", file_path.as_ref().display(), err);
      return;
    }
  };
  if let Err(err) = environment.atomic_write_file_bytes(&file_path, json_text.as_bytes()) {
    log_warn!(environment, "Error saving incremental file {}: {}", file_path.as_ref().display(), err);
  }
}
