use std::path::PathBuf;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{action} {path}: {source}")]
    Io {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("not a PoB2 install: {0}")]
    NotAnInstall(String),

    #[error("PoB2 install is corrupt: {0}")]
    Corrupt(String),

    #[error("download failed: {0}")]
    Download(String),

    #[error("invalid build code: {0}")]
    Code(String),

    #[error("invalid build XML: {0}")]
    Xml(String),

    #[error("PoB failed to start: {0}")]
    Startup(String),

    #[error("PoB reported an error: {0}")]
    Pob(String),

    /// PoB's output kept changing between recalculations, so no single
    /// answer exists. Seen with PoB bugs where a calculation feeds back into
    /// the build (for example an item line rewritten on every pass).
    #[error("PoB's numbers never settled: they changed on each of {passes} recalculations. Still changing:\n  {}", .changing.join("\n  "))]
    Unsettled { passes: u32, changing: Vec<String> },

    /// Kept as text so `Error` is `Send + Sync` (mlua's error is not).
    #[error("Lua error: {0}")]
    Lua(String),
}

impl From<mlua::Error> for Error {
    fn from(err: mlua::Error) -> Self {
        Self::Lua(err.to_string())
    }
}

impl Error {
    pub(crate) fn io(
        action: &'static str,
        path: impl Into<PathBuf>,
        source: std::io::Error,
    ) -> Self {
        Self::Io {
            action,
            path: path.into(),
            source,
        }
    }
}
