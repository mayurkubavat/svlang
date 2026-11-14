//! The crate's error type, and the few helpers the readers use to build one.

/// Why a model could not be read or lowered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

pub(crate) type Result<T> = std::result::Result<T, Error>;

macro_rules! anyhow {
    ($($t:tt)*) => { $crate::err::Error(format!($($t)*)) };
}
macro_rules! bail {
    ($($t:tt)*) => { return Err($crate::err::Error(format!($($t)*))) };
}
pub(crate) use {anyhow, bail};

/// `context` on a `Result` or `Option`, as a prefix of the message.
pub(crate) trait Context<T> {
    fn context(self, msg: &str) -> Result<T>;
}

impl<T, E: std::fmt::Display> Context<T> for std::result::Result<T, E> {
    fn context(self, msg: &str) -> Result<T> {
        self.map_err(|e| Error(format!("{msg}: {e}")))
    }
}

impl From<std::num::ParseIntError> for Error {
    fn from(e: std::num::ParseIntError) -> Self {
        Error(format!("bad number: {e}"))
    }
}
