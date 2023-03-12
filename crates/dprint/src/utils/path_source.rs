use std::fmt;

use url::Url;

use crate::environment::CanonicalizedPathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PluginKind {
  Process,
  Wasm,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PathSource {
  /// From the local file system.
  Local(LocalPathSource),
  /// From the internet.
  Remote(RemotePathSource),
}

impl PathSource {
  pub fn new_local(path: CanonicalizedPathBuf) -> PathSource {
    PathSource::Local(LocalPathSource { path })
  }

  pub fn new_remote(url: Url) -> PathSource {
    PathSource::Remote(RemotePathSource { url })
  }

  #[cfg(test)]
  pub fn new_remote_from_str(url: &str) -> PathSource {
    PathSource::Remote(RemotePathSource { url: Url::parse(url).unwrap() })
  }

  pub fn parent(&self) -> PathSource {
    match self {
      PathSource::Local(local) => {
        if let Some(parent) = local.path.parent() {
          PathSource::new_local(parent)
        } else {
          PathSource::new_local(local.path.clone())
        }
      }
      PathSource::Remote(remote) => {
        let mut parent_url = remote.url.join("./").expect("Expected to be able to go back a directory in the url.");
        parent_url.set_query(None);
        PathSource::new_remote(parent_url)
      }
    }
  }

  pub fn unwrap_local(&self) -> LocalPathSource {
    if let PathSource::Local(local_path_source) = self {
      local_path_source.clone()
    } else {
      panic!("Attempted to unwrap a path source as local that was not local.");
    }
  }

  pub fn unwrap_remote(&self) -> RemotePathSource {
    if let PathSource::Remote(remote_path_source) = self {
      remote_path_source.clone()
    } else {
      panic!("Attempted to unwrap a path source as remote that was not remote.");
    }
  }

  pub fn display(&self) -> String {
    match self {
      PathSource::Local(local) => local.path.display().to_string(),
      PathSource::Remote(remote) => remote.url.to_string(),
    }
  }

  pub fn plugin_kind(&self) -> Option<PluginKind> {
    let lowercase_path = self.display().to_lowercase();
    if lowercase_path.ends_with(".wasm") {
      Some(PluginKind::Wasm)
    } else if lowercase_path.ends_with(".json") {
      Some(PluginKind::Process)
    } else {
      None
    }
  }
}

impl fmt::Display for PathSource {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(
      f,
      "{}",
      match self {
        PathSource::Local(local) => local.path.to_string_lossy().to_string(),
        PathSource::Remote(remote) => remote.url.to_string(),
      }
    )
  }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LocalPathSource {
  pub path: CanonicalizedPathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RemotePathSource {
  pub url: Url,
}

#[cfg(test)]
mod tests {
  use super::*;
  use url::Url;

  #[test]
  fn should_get_parent_for_url() {
    let source = PathSource::new_remote(Url::parse("https://dprint.dev/test/test.json").unwrap());
    let parent = source.parent();
    assert_eq!(parent, PathSource::new_remote(Url::parse("https://dprint.dev/test/").unwrap()))
  }

  #[test]
  fn should_get_parent_for_file_path() {
    let source = PathSource::new_local(CanonicalizedPathBuf::new_for_testing("/test/test/asdf.json"));
    let parent = source.parent();
    assert_eq!(parent, PathSource::new_local(CanonicalizedPathBuf::new_for_testing("/test/test")))
  }

  #[test]
  fn should_get_parent_for_root_dir_file() {
    let source = PathSource::new_local(CanonicalizedPathBuf::new_for_testing("/test.json"));
    let parent = source.parent();
    assert_eq!(parent, PathSource::new_local(CanonicalizedPathBuf::new_for_testing("/")))
  }

  #[test]
  fn should_get_parent_for_root_dir() {
    let source = PathSource::new_local(CanonicalizedPathBuf::new_for_testing("/"));
    let parent = source.parent();
    assert_eq!(parent, PathSource::new_local(CanonicalizedPathBuf::new_for_testing("/")))
  }
}
