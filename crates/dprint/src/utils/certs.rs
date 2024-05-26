use std::io::Cursor;

use indexmap::IndexSet;
use rustls::pki_types::CertificateDer;
use rustls::RootCertStore;
use thiserror::Error;

/// Much of this code lifted and adapted from https://github.com/denoland/deno/blob/5de30c53239ac74843725d981afc0bb8c45bdf16/cli/args/mod.rs#L600
/// Copyright the Deno authors. MIT License.

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum RootCertStoreLoadError {
  #[error("Unknown certificate store \"{0}\" specified (allowed: \"system,mozilla\")")]
  UnknownStore(String),
  #[error("Unable to add pem file to certificate store: {0}")]
  FailedAddPemFile(String),
  #[error("Failed opening CA file: {0}")]
  CaFileOpenError(String),
}

pub fn get_root_cert_store(
  read_env_var: &impl Fn(&str) -> Option<String>,
  read_file_bytes: &impl Fn(&str) -> Result<Vec<u8>, std::io::Error>,
) -> Result<RootCertStore, RootCertStoreLoadError> {
  let cert_info = load_cert_info(read_env_var, read_file_bytes)?;
  Ok(create_root_cert_store(cert_info))
}

struct CertInfo {
  ca_stores: Vec<CaStore>,
  ca_file: Option<Vec<CertificateDer<'static>>>,
}

fn load_cert_info(
  read_env_var: &impl Fn(&str) -> Option<String>,
  read_file_bytes: &impl Fn(&str) -> Result<Vec<u8>, std::io::Error>,
) -> Result<CertInfo, RootCertStoreLoadError> {
  Ok(CertInfo {
    ca_stores: parse_ca_stores(read_env_var)?,
    ca_file: match read_env_var("DPRINT_CERT") {
      Some(ca_file) if !ca_file.trim().is_empty() => {
        let certs = load_certs_from_file(&ca_file, read_file_bytes)?;
        Some(certs)
      }
      _ => None,
    },
  })
}

fn create_root_cert_store(info: CertInfo) -> RootCertStore {
  let mut root_cert_store = RootCertStore::empty();

  for store in info.ca_stores {
    load_store(store, &mut root_cert_store);
  }

  if let Some(ca_file) = info.ca_file {
    root_cert_store.add_parsable_certificates(ca_file.into_iter());
  }

  root_cert_store
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
enum CaStore {
  System,
  Mozilla,
}

fn load_store(store: CaStore, root_cert_store: &mut RootCertStore) {
  match store {
    CaStore::Mozilla => {
      root_cert_store
        .roots
        .extend(webpki_roots::TLS_SERVER_ROOTS.iter().map(|ta| rustls::pki_types::TrustAnchor {
          subject: ta.subject.into(),
          subject_public_key_info: ta.spki.into(),
          name_constraints: ta.name_constraints.map(|n| n.into()),
        }));
    }
    CaStore::System => {
      let roots = rustls_native_certs::load_native_certs().expect("could not load platform certs");
      for root in roots {
        root_cert_store.add(root).expect("Failed to add platform cert to root cert store");
      }
    }
  }
}

fn parse_ca_stores(read_env_var: &impl Fn(&str) -> Option<String>) -> Result<Vec<CaStore>, RootCertStoreLoadError> {
  let Some(env_ca_store) = read_env_var("DPRINT_TLS_CA_STORE") else {
    return Ok(vec![CaStore::Mozilla, CaStore::System]);
  };

  let mut values = IndexSet::with_capacity(2);
  for value in env_ca_store.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
    match value {
      "system" => {
        values.insert(CaStore::System);
      }
      "mozilla" => {
        values.insert(CaStore::Mozilla);
      }
      _ => {
        return Err(RootCertStoreLoadError::UnknownStore(value.to_string()));
      }
    }
  }
  Ok(values.into_iter().collect())
}

fn load_certs_from_file(
  file_path: &str,
  read_file_bytes: &impl Fn(&str) -> Result<Vec<u8>, std::io::Error>,
) -> Result<Vec<CertificateDer<'static>>, RootCertStoreLoadError> {
  let certfile = read_file_bytes(file_path).map_err(|err| RootCertStoreLoadError::CaFileOpenError(err.to_string()))?;
  let mut reader = Cursor::new(certfile);
  let mut data = Vec::new();
  for result in rustls_pemfile::certs(&mut reader) {
    let cert = result.map_err(|e| RootCertStoreLoadError::FailedAddPemFile(e.to_string()))?;
    data.push(cert);
  }
  Ok(data)
}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  fn parses_ca_stores() {
    let test_cases = vec![
      ("mozilla", Ok(vec![CaStore::Mozilla])),
      ("system", Ok(vec![CaStore::System])),
      ("mozilla,system", Ok(vec![CaStore::Mozilla, CaStore::System])),
      ("mozilla,system,mozilla,system", Ok(vec![CaStore::Mozilla, CaStore::System])),
      ("system,mozilla", Ok(vec![CaStore::System, CaStore::Mozilla])),
      ("  system  ,  mozilla,  , ,,", Ok(vec![CaStore::System, CaStore::Mozilla])),
      ("system,mozilla,other", Err(RootCertStoreLoadError::UnknownStore("other".to_string()))),
    ];
    for (input, expected) in test_cases {
      let actual = parse_ca_stores(&move |var_name| {
        assert_eq!(var_name, "DPRINT_TLS_CA_STORE");
        Some(input.to_string())
      });
      assert_eq!(actual, expected);
    }
  }

  const ROOT_CA: &[u8] = b"-----BEGIN CERTIFICATE-----
MIIDIzCCAgugAwIBAgIJAMKPPW4tsOymMA0GCSqGSIb3DQEBCwUAMCcxCzAJBgNV
BAYTAlVTMRgwFgYDVQQDDA9FeGFtcGxlLVJvb3QtQ0EwIBcNMTkxMDIxMTYyODIy
WhgPMjExODA5MjcxNjI4MjJaMCcxCzAJBgNVBAYTAlVTMRgwFgYDVQQDDA9FeGFt
cGxlLVJvb3QtQ0EwggEiMA0GCSqGSIb3DQEBAQUAA4IBDwAwggEKAoIBAQDMH/IO
2qtHfyBKwANNPB4K0q5JVSg8XxZdRpTTlz0CwU0oRO3uHrI52raCCfVeiQutyZop
eFZTDWeXGudGAFA2B5m3orWt0s+touPi8MzjsG2TQ+WSI66QgbXTNDitDDBtTVcV
5G3Ic+3SppQAYiHSekLISnYWgXLl+k5CnEfTowg6cjqjVr0KjL03cTN3H7b+6+0S
ws4rYbW1j4ExR7K6BFNH6572yq5qR20E6GqlY+EcOZpw4CbCk9lS8/CWuXze/vMs
OfDcc6K+B625d27wyEGZHedBomT2vAD7sBjvO8hn/DP1Qb46a8uCHR6NSfnJ7bXO
G1igaIbgY1zXirNdAgMBAAGjUDBOMB0GA1UdDgQWBBTzut+pwwDfqmMYcI9KNWRD
hxcIpTAfBgNVHSMEGDAWgBTzut+pwwDfqmMYcI9KNWRDhxcIpTAMBgNVHRMEBTAD
AQH/MA0GCSqGSIb3DQEBCwUAA4IBAQB9AqSbZ+hEglAgSHxAMCqRFdhVu7MvaQM0
P090mhGlOCt3yB7kdGfsIrUW6nQcTz7PPQFRaJMrFHPvFvPootkBUpTYR4hTkdce
H6RCRu2Jxl4Y9bY/uezd9YhGCYfUtfjA6/TH9FcuZfttmOOlxOt01XfNvVMIR6RM
z/AYhd+DeOXjr35F/VHeVpnk+55L0PYJsm1CdEbOs5Hy1ecR7ACuDkXnbM4fpz9I
kyIWJwk2zJReKcJMgi1aIinDM9ao/dca1G99PHOw8dnr4oyoTiv8ao6PWiSRHHMi
MNf4EgWfK+tZMnuqfpfO9740KzfcVoMNo4QJD4yn5YxroUOO/Azi
-----END CERTIFICATE-----";

  #[test]
  fn load_cert_file_success() {
    let result = load_certs_from_file("path.pem", &|path| {
      assert_eq!(path, "path.pem");
      Ok(ROOT_CA.to_vec())
    })
    .unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].len(), 807);
  }

  #[test]
  fn load_cert_file_not_found() {
    let err = load_certs_from_file("not_found.pem", &|path| {
      Err(std::io::Error::new(std::io::ErrorKind::NotFound, format!("not found '{}'", path)))
    })
    .err()
    .unwrap();
    let err = match err {
      RootCertStoreLoadError::CaFileOpenError(e) => e,
      _ => unreachable!(),
    };
    assert_eq!(err, "not found 'not_found.pem'");
  }

  #[test]
  fn loads_cert_info() {
    let info = load_cert_info(
      &|var| match var {
        "DPRINT_TLS_CA_STORE" => Some("mozilla".to_string()),
        "DPRINT_CERT" => Some("path.pem".to_string()),
        _ => None,
      },
      &|path| {
        assert_eq!(path, "path.pem");
        Ok(ROOT_CA.to_vec())
      },
    )
    .unwrap();
    assert_eq!(info.ca_stores, vec![CaStore::Mozilla]);
    assert_eq!(info.ca_file.unwrap()[0].len(), 807);
  }
}
