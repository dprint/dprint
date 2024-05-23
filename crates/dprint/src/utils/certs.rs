use std::io::Cursor;

use deno_native_certs::load_native_certs;
use rustls::RootCertStore;
use thiserror::Error;

/// Much of this code lifted and adapted from https://github.com/denoland/deno/blob/5de30c53239ac74843725d981afc0bb8c45bdf16/cli/args/mod.rs#L600
/// Copyright the Deno authors. MIT License.

#[derive(Error, Debug, Clone)]
pub enum RootCertStoreLoadError {
  #[error("Unknown certificate store \"{0}\" specified (allowed: \"system,mozilla\")")]
  UnknownStore(String),
  #[error("Unable to add pem file to certificate store: {0}")]
  FailedAddPemFile(String),
  #[error("Failed opening CA file: {0}")]
  CaFileOpenError(String),
}

pub fn get_root_cert_store(
  read_env_var: impl Fn(&str) -> Option<String>,
  read_file_bytes: impl Fn(&str) -> Result<Vec<u8>, std::io::Error>,
) -> Result<RootCertStore, RootCertStoreLoadError> {
  let mut root_cert_store = RootCertStore::empty();
  let ca_stores: Vec<String> = read_env_var("DPRINT_TLS_CA_STORE")
    .map(|env_ca_store| env_ca_store.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
    .unwrap_or_else(|| vec!["system".to_string()]);

  for store in ca_stores {
    match store.as_str() {
      "mozilla" => {
        root_cert_store.add_trust_anchors(
          webpki_roots::TLS_SERVER_ROOTS
            .iter()
            .map(|ta| rustls::OwnedTrustAnchor::from_subject_spki_name_constraints(ta.subject, ta.spki, ta.name_constraints)),
        );
      }
      "system" => {
        let roots = load_native_certs().expect("could not load platform certs");
        for root in roots {
          root_cert_store
            .add(&rustls::Certificate(root.0))
            .expect("Failed to add platform cert to root cert store");
        }
      }
      _ => {
        return Err(RootCertStoreLoadError::UnknownStore(store));
      }
    }
  }

  if let Some(ca_file) = read_env_var("DPRINT_CERT") {
    let result = {
      let certfile = read_file_bytes(&ca_file).map_err(|err| RootCertStoreLoadError::CaFileOpenError(err.to_string()))?;
      let mut reader = Cursor::new(certfile);
      rustls_pemfile::certs(&mut reader)
    };

    match result {
      Ok(certs) => {
        root_cert_store.add_parsable_certificates(&certs);
      }
      Err(e) => {
        return Err(RootCertStoreLoadError::FailedAddPemFile(e.to_string()));
      }
    }
  }

  Ok(root_cert_store)
}
