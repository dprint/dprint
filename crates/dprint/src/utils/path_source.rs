
use std::path::PathBuf;
use url::Url;

#[derive(Clone, Debug, PartialEq)]
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
}

#[derive(Clone, Debug, PartialEq)]
pub struct LocalPathSource {
    pub(super) path: PathBuf,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RemotePathSource {
    pub(super) url: Url,
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