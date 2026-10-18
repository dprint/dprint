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
  /// The data read from the existing file, when it exists and was
  /// created with the same plugins.
  read_data: Option<IncrementalFileData>,
  write_data: Mutex<IncrementalFileData>,
  /// Whether the run only covers some of the files, in which case the hashes
  /// of the files not seen this run are kept when writing.
  is_partial_run: bool,
  environment: TEnvironment,
}

impl<TEnvironment: Environment> IncrementalFile<TEnvironment> {
  pub fn new(file_path: CanonicalizedPathBuf, plugins_hash: u64, is_partial_run: bool, environment: TEnvironment) -> Self {
    let read_data = read_incremental(&file_path, &environment).and_then(|read_data| {
      if read_data.plugins_hash == plugins_hash {
        Some(read_data)
      } else {
        log_debug!(environment, "Plugins changed. Creating new incremental file.");
        None
      }
    });
    IncrementalFile {
      file_path,
      read_data,
      write_data: Mutex::new(IncrementalFileData::new(plugins_hash)),
      is_partial_run,
      environment,
    }
  }

  /// If the file text is known to be formatted.
  pub fn is_file_known_formatted(&self, file_text: &[u8]) -> bool {
    let hash = get_bytes_hash(file_text);
    if self.read_data.as_ref().is_some_and(|data| data.file_hashes.contains(&hash)) {
      // the file is the same, so save it in the write data
      self.add_to_write_data(hash);
      true
    } else {
      false
    }
  }

  pub fn update_file(&self, file_text: &[u8]) {
    let hash = get_bytes_hash(file_text);
    self.add_to_write_data(hash)
  }

  fn add_to_write_data(&self, hash: u64) {
    let mut write_data = self.write_data.lock();
    write_data.file_hashes.insert(hash);
  }

  pub fn write(&self) {
    let write_data = self.write_data.lock();
    if let Some(read_data) = &self.read_data {
      // don't rewrite the file when nothing new was learned, which is the case
      // when every file seen this run was already known to be formatted; this
      // keeps the file's bytes stable so a CI cache of the cache directory can
      // detect it hasn't changed
      if write_data.file_hashes.is_subset(&read_data.file_hashes) {
        log_debug!(self.environment, "Incremental file unchanged. Skipping write.");
        return;
      }
      // a partial run keeps the hashes of the files it didn't see, while a full
      // run writes only what it saw so the hashes of changed and deleted files
      // get pruned
      if self.is_partial_run {
        let merged_data = IncrementalFileData {
          plugins_hash: write_data.plugins_hash,
          file_hashes: write_data.file_hashes.union(&read_data.file_hashes).copied().collect(),
        };
        write_incremental(&self.file_path, &merged_data, &self.environment);
        return;
      }
    }
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
