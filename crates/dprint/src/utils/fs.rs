// Lifted from some Deno code I wrote.
// Copyright the Deno authors. MIT license.

use std::io::Error;
use std::io::ErrorKind;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use sys_traits::FsCreateDirAll;
use sys_traits::FsMetadata;
use sys_traits::FsOpen;
use sys_traits::FsRemoveFile;
use sys_traits::FsRename;
use sys_traits::OpenOptions;
use sys_traits::SystemRandom;
use sys_traits::ThreadSleep;

pub fn get_atomic_path(sys: &impl SystemRandom, path: &Path) -> PathBuf {
  let rand = gen_rand_path_component(sys);
  let extension = format!("{rand}.tmp");
  path.with_extension(extension)
}

fn gen_rand_path_component(sys: &impl SystemRandom) -> String {
  use std::fmt::Write;
  (0..4).fold(String::with_capacity(8), |mut output, _| {
    write!(&mut output, "{:02x}", sys.sys_random_u8().unwrap()).unwrap();
    output
  })
}

#[sys_traits::auto_impl]
pub trait AtomicWriteFileWithRetriesSys: AtomicWriteFileSys + ThreadSleep {}

pub fn atomic_write_file_with_retries<TSys: AtomicWriteFileWithRetriesSys>(sys: &TSys, file_path: &Path, data: &[u8], mode: u32) -> std::io::Result<()> {
  let mut count = 0;
  loop {
    match atomic_write_file(sys, file_path, data, mode) {
      Ok(()) => return Ok(()),
      Err(err) => {
        if count >= 5 {
          // too many retries, return the error
          return Err(err);
        }
        count += 1;
        let sleep_ms = std::cmp::min(50, 10 * count);
        sys.thread_sleep(std::time::Duration::from_millis(sleep_ms));
      }
    }
  }
}

#[sys_traits::auto_impl]
pub trait AtomicWriteFileSys: FsCreateDirAll + FsMetadata + FsOpen + FsRemoveFile + FsRename + SystemRandom {}

/// Writes the file to the file system at a temporary path, then
/// renames it to the destination in a single sys call in order
/// to never leave the file system in a corrupted state.
///
/// This also handles creating the directory if a NotFound error
/// occurs.
pub fn atomic_write_file<TSys: AtomicWriteFileSys>(sys: &TSys, file_path: &Path, data: &[u8], mode: u32) -> std::io::Result<()> {
  fn atomic_write_file_raw<TSys: AtomicWriteFileSys>(sys: &TSys, temp_file_path: &Path, file_path: &Path, data: &[u8], mode: u32) -> std::io::Result<()> {
    let mut options = OpenOptions::new_write();
    options.mode = Some(mode);
    let mut file = sys.fs_open(temp_file_path, &options)?;
    file.write_all(data)?;
    sys.fs_rename(temp_file_path, file_path).inspect_err(|_err| {
      // clean up the created temp file on error
      let _ = sys.fs_remove_file(temp_file_path);
    })
  }

  let temp_file_path = get_atomic_path(sys, file_path);

  if let Err(write_err) = atomic_write_file_raw(sys, &temp_file_path, file_path, data, mode) {
    if write_err.kind() == ErrorKind::NotFound {
      let parent_dir_path = file_path.parent().unwrap();
      match sys.fs_create_dir_all(parent_dir_path) {
        Ok(()) => {
          return atomic_write_file_raw(sys, &temp_file_path, file_path, data, mode).map_err(|err| add_file_context_to_err(file_path, err));
        }
        Err(create_err) => {
          if !sys.fs_exists(parent_dir_path).unwrap_or(false) {
            return Err(Error::new(
              create_err.kind(),
              format!("{:#} (for '{}')\nCheck the permission of the directory.", create_err, parent_dir_path.display()),
            ));
          }
        }
      }
    }
    return Err(add_file_context_to_err(file_path, write_err));
  }
  Ok(())
}

fn add_file_context_to_err(file_path: &Path, err: Error) -> Error {
  Error::new(err.kind(), format!("{:#} (for '{}')", err, file_path.display()))
}
