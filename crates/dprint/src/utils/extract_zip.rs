use std::path::PathBuf;
use std::io::prelude::*;

use crate::environment::Environment;
use crate::types::ErrBox;

pub fn extract_zip(zip_bytes: &[u8], dir_path: &PathBuf, environment: &impl Environment) -> Result<(), ErrBox> {
    // adapted from https://github.com/mvdnes/zip-rs/blob/master/examples/extract.rs
    let mut reader = std::io::Cursor::new(&zip_bytes);
    let mut zip = zip::ZipArchive::new(reader)?;

    // todo: consider parallelizing this
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).unwrap();
        let file_name = file.sanitized_name();
        let file_path = dir_path.join(file_name);

        if !file.is_dir() {
            if let Some(parent_dir_path) = file_path.parent() {
                environment.mk_dir_all(&parent_dir_path.to_path_buf());
            }
            let mut file_bytes = Vec::with_capacity(file.size() as usize);
            file.read_to_end(&mut file_bytes)?;
            environment.write_file_bytes(&file_path, &file_bytes)?;
        }

        // Get and Set permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            if let Some(mode) = file.unix_mode() {
                fs::set_permissions(&outpath, fs::Permissions::from_mode(mode)).unwrap();
            }
        }
    }

    Ok(())
}