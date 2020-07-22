use std::error::Error as StdError;

pub type ErrBox = Box<dyn StdError + Send + Sync>;

#[derive(std::fmt::Debug)]
pub struct Error(String);

impl Error {
    pub fn new(text: String) -> Box<Self> {
        Box::new(Error(text))
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl StdError for Error {}

#[macro_export]
macro_rules! err {
    ($($arg:tt)*) => {
        Err($crate::types::Error::new(format!($($arg)*)));
    }
}
