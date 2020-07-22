use std::error::Error as StdError;

pub type ErrBox = Box<dyn StdError + Send + Sync>;
