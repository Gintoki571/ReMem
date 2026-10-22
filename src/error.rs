use std::fmt;

/// One error type for the whole crate: a message an agent can read.
#[derive(Debug)]
pub struct Error(pub String);

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

/// `err!("bad kind {k}")` -> `Error`
#[macro_export]
macro_rules! err {
    ($($t:tt)*) => { $crate::error::Error(format!($($t)*)) };
}

macro_rules! from {
    ($($t:ty),*) => {$(
        impl From<$t> for Error {
            fn from(e: $t) -> Self { Error(e.to_string()) }
        }
    )*};
}
from!(
    rusqlite::Error,
    graphqlite::Error,
    candle_core::Error,
    std::io::Error,
    serde_json::Error
);

impl From<tokenizers::Error> for Error {
    fn from(e: tokenizers::Error) -> Self {
        Error(e.to_string())
    }
}
