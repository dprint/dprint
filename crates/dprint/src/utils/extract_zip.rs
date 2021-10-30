use std::io::prelude::*;
use std::path::Path;

use dprint_core::types::ErrBox;

use crate::environment::Environment;

pub fn extract_zip(message: &str, zip_bytes: &[u8], dir_path: &Path, environment: &impl Environment) -> Result<(), ErrBox> {
  // adapted from https://github.com/mvdnes/zip-rs/blob/master/examples/extract.rs
  let reader = std::io::Cursor::new(&zip_bytes);
  let mut zip = zip::ZipArchive::new(reader)?;
  let length = zip.len();

  log_verbose!(environment, "Extracting zip file to directory: {}", dir_path.display());

  environment.log_action_with_progress(
    message,
    move |update_size| -> Result<(), ErrBox> {
      // todo: consider parallelizing this
      for i in 0..zip.len() {
        update_size(i);
        let mut file = zip.by_index(i).unwrap();
        if let Some(file_name) = file.enclosed_name() {
          let file_path = dir_path.join(file_name);

          if !file.is_dir() {
            if let Some(parent_dir_path) = file_path.parent() {
              environment.mk_dir_all(&parent_dir_path.to_path_buf())?;
            }
            let mut file_bytes = Vec::with_capacity(file.size() as usize);
            file.read_to_end(&mut file_bytes)?;
            environment.write_file_bytes(&file_path, &file_bytes)?;
          } else {
            environment.mk_dir_all(&file_path)?;
          }

          // Get and Set permissions
          #[cfg(unix)]
          if environment.is_real() {
            use std::fs;
            use std::os::unix::fs::PermissionsExt;

            if let Some(mode) = file.unix_mode() {
              fs::set_permissions(&file_path, fs::Permissions::from_mode(mode))
                .map_err(|err| err_obj!("Error setting permissions to {} for file {}: {}", mode, file_path.display(), err))?
            }
          }
        } else {
          environment.log_stderr(&format!("Ignoring path in zip because it was not enclosed: {}", file.name()));
        }
      }

      Ok(())
    },
    length,
  )?;

  Ok(())
}
