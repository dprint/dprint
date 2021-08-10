
use std::path::PathBuf;
use url::Url;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PathSource {
    /// From the local file system.
    Local(LocalPathSource),
    /// From the internet.
    Remote(RemotePathSource)
}

impl PathSource {
    pub fn new_local(path: PathBuf) -> PathSource {
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
                    PathSource::new_local(PathBuf::from(parent))
                } else {
                    PathSource::new_local(local.path.clone())
                }
            },
            PathSource::Remote(remote) => {
                let mut parent_url = remote.url.join("./").expect("Expected to be able to go back a directory in the url.");
                parent_url.set_query(None);
                PathSource::new_remote(parent_url)
            },
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
            PathSource::Local(local) => {
                local.path.display().to_string()
            },
            PathSource::Remote(remote) => {
                remote.url.to_string()
            },
        }
    }

    pub fn is_wasm_plugin(&self) -> bool {
        self.display().to_lowercase().ends_with(".wasm")
    }

    pub fn is_process_plugin(&self) -> bool {
        self.display().to_lowercase().ends_with(".exe-plugin")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LocalPathSource {
    pub path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RemotePathSource {
    pub url: Url,
}

#[cfg(test)]
mod tests {
    use url::Url;
    use super::*;

    #[test]
    fn it_should_get_parent_for_url() {
        let source = PathSource::new_remote(Url::parse("https://dprint.dev/test/test.json").unwrap());
        let parent = source.parent();
        assert_eq!(parent, PathSource::new_remote(Url::parse("https://dprint.dev/test/").unwrap()))
    }

    #[test]
    fn it_should_get_parent_for_file_path() {
        let source = PathSource::new_local(PathBuf::from("/test/test/asdf.json"));
        let parent = source.parent();
        assert_eq!(parent, PathSource::new_local(PathBuf::from("/test/test")))
    }

    #[test]
    fn it_should_get_parent_for_root_dir_file() {
        let source = PathSource::new_local(PathBuf::from("/test.json"));
        let parent = source.parent();
        assert_eq!(parent, PathSource::new_local(PathBuf::from("/")))
    }

    #[test]
    fn it_should_get_parent_for_root_dir() {
        let source = PathSource::new_local(PathBuf::from("/"));
        let parent = source.parent();
        assert_eq!(parent, PathSource::new_local(PathBuf::from("/")))
    }
}