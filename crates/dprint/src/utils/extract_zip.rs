use std::path::PathBuf;
use std::io::prelude::*;

use dprint_core::types::ErrBox;

use crate::environment::Environment;

pub async fn extract_zip(message: &str, zip_bytes: &[u8], dir_path: &PathBuf, environment: &impl Environment) -> Result<(), ErrBox> {
    // adapted from https://github.com/mvdnes/zip-rs/blob/master/examples/extract.rs
    let reader = std::io::Cursor::new(&zip_bytes);
    let mut zip = zip::ZipArchive::new(reader)?;
    let length = zip.len();

    environment.log_action_with_progress(message, move |update_size| -> Result<(), ErrBox> {
        // todo: consider parallelizing this
        for i in 0..zip.len() {
            update_size(i);
            let mut file = zip.by_index(i).unwrap();
            let file_name = file.sanitized_name();
            let file_path = dir_path.join(file_name);

            if !file.is_dir() {
                if let Some(parent_dir_path) = file_path.parent() {
                    environment.mk_dir_all(&parent_dir_path.to_path_buf())?;
                }
                let mut file_bytes = Vec::with_capacity(file.size() as usize);
                file.read_to_end(&mut file_bytes)?;
                environment.write_file_bytes(&file_path, &file_bytes)?;
            }

            // Get and Set permissions
            #[cfg(unix)]
            if environment.is_real() {
                use std::os::unix::fs::PermissionsExt;
                use std::fs;

                if let Some(mode) = file.unix_mode() {
                    fs::set_permissions(&file_path, fs::Permissions::from_mode(mode)).unwrap();
                }
            }
        }

        Ok(())
    }, length).await??;

    Ok(())
}
