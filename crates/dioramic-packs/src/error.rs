//! Errors of the pack store.

use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

/// Something went wrong reading, installing or removing a pack.
#[derive(Debug, Error)]
pub enum PackError {
    /// Filesystem error.
    #[error("{path}: {source}")]
    Io {
        /// File or directory involved.
        path: PathBuf,
        /// Cause.
        source: io::Error,
    },
    /// Unreadable archive.
    #[error("{path}: not a readable zip archive ({message})")]
    Zip {
        /// Archive.
        path: PathBuf,
        /// Cause.
        message: String,
    },
    /// Invalid `manifest.json`.
    #[error("{path}: invalid manifest ({message})")]
    Manifest {
        /// Manifest file.
        path: PathBuf,
        /// What is wrong.
        message: String,
    },
    /// Neither `XDG_DATA_HOME` nor `HOME` (nor `LOCALAPPDATA`) is set.
    #[error("cannot find the data directory: set XDG_DATA_HOME or HOME")]
    NoDataDir,
    /// The sources had no icon of the provider's layout.
    #[error("no {provider} icon found in {origin}")]
    NoIcons {
        /// Provider id.
        provider: String,
        /// What was read.
        origin: String,
    },
    /// Too many files (protects against archive bombs).
    #[error("{origin} has more than {limit} files; is it the right archive?")]
    TooManyFiles {
        /// What was read.
        origin: String,
        /// Limit.
        limit: usize,
    },
    /// A pack id that is not installed.
    #[error("icon pack '{0}' is not installed")]
    NotInstalled(String),
}

impl PackError {
    /// [`PackError::Io`] for `path`.
    pub fn io(path: &Path, source: io::Error) -> Self {
        Self::Io { path: path.to_path_buf(), source }
    }
}
