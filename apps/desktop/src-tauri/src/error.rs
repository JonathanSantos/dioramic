//! Errors returned by Tauri commands (serialized to the frontend as their message).

use std::path::Path;

use serde::{Serialize, Serializer};
use thiserror::Error;

use crate::pin::PinError;

/// Why a command failed.
#[derive(Debug, Error)]
pub enum CommandError {
    /// The path is not a folder.
    #[error("not a folder: {0}")]
    NotAFolder(String),
    /// The path is not a `*.diorama.*` file; the app refuses to read or write anything else.
    #[error("not a Diorama spec file: {0}")]
    NotASpec(String),
    /// File system error.
    #[error("{path}: {message}")]
    Io {
        /// Path involved.
        path: String,
        /// OS message.
        message: String,
    },
    /// The engine failed (render errors, invalid spec).
    #[error("{0}")]
    Engine(String),
    /// The file watcher could not start.
    #[error("cannot watch the folder: {0}")]
    Watch(String),
    /// The view cannot be pinned into the spec text.
    #[error("{0}")]
    Pin(#[from] PinError),
    /// A background task died.
    #[error("background task failed: {0}")]
    Task(String),
}

impl CommandError {
    /// I/O error on `path`.
    pub fn io(path: &Path, error: &std::io::Error) -> Self {
        Self::Io { path: path.display().to_string(), message: error.to_string() }
    }
}

impl Serialize for CommandError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}
