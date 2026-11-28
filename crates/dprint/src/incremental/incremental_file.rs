use parking_lot::Mutex;
use std::collections::HashSet;
use std::path::Path;

use crate::environment::CanonicalizedPathBuf;
use crate::environment::Environment;
use crate::utils::get_bytes_hash;

/// The version of the incremental file's format, which is included in its
/// file name so a file in a different format is never read. Before this was
/// versioned, the file was JSON and didn't have the version in its name.
pub const INCREMENTAL_FILE_VERSION: u32 = 1;

const HASH_SIZE: usize = std::mem::size_of::<u64>();

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

  /// Serializes the data as the plugins hash followed by the file hashes,
  /// each a little endian `u64`. The file hashes are sorted so the bytes are
  /// the same for the same data.
  fn to_bytes(&self) -> Vec<u8> {
    let mut file_hashes = self.file_hashes.iter().copied().collect::<Vec<_>>();
    file_hashes.sort_unstable();
    let mut bytes = Vec::with_capacity((file_hashes.len() + 1) * HASH_SIZE);
    bytes.extend_from_slice(&self.plugins_hash.to_le_bytes());
    for hash in file_hashes {
      bytes.extend_from_slice(&hash.to_le_bytes());
    }
    bytes
  }

  fn from_bytes(bytes: &[u8]) -> Option<IncrementalFileData> {
    if bytes.len() < HASH_SIZE || !bytes.len().is_multiple_of(HASH_SIZE) {
      return None;
    }
    let mut hashes = bytes.chunks_exact(HASH_SIZE).map(|chunk| u64::from_le_bytes(chunk.try_into().unwrap()));
    let plugins_hash = hashes.next()?;
    Some(IncrementalFileData {
      plugins_hash,
      file_hashes: hashes.collect(),
    })
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
  let bytes = match environment.read_file_bytes(&file_path) {
    Ok(bytes) => bytes,
    Err(err) => {
      if environment.path_exists(&file_path) {
        log_warn!(environment, "Error reading incremental file {}: {}", file_path.as_ref().display(), err);
      }
      return None;
    }
  };

  let file_data = IncrementalFileData::from_bytes(&bytes);
  if file_data.is_none() {
    log_warn!(
      environment,
      "Error reading incremental file {}: The file is corrupt.",
      file_path.as_ref().display()
    );
  }
  file_data
}

fn write_incremental(file_path: impl AsRef<Path>, file_data: &IncrementalFileData, environment: &impl Environment) {
  if let Err(err) = environment.atomic_write_file_bytes(&file_path, &file_data.to_bytes()) {
    log_warn!(environment, "Error saving incremental file {}: {}", file_path.as_ref().display(), err);
  }
}
