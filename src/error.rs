//! Error types for RustyWrapper.
//!
//! This module provides consolidated error types for configuration and runtime errors.

use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur during configuration.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// The specified Python directory does not exist.
    #[error("Python directory not found: {0}")]
    PythonDirNotFound(PathBuf),

    /// The rustywrapper.py framework file was not found in the Python directory.
    #[error("Framework file 'rustywrapper.py' not found in: {0}")]
    FrameworkNotFound(PathBuf),

    /// Error resolving a path.
    #[error("Path resolution error: {0}")]
    PathResolution(String),

    /// Invalid worker count configuration.
    #[error("Invalid worker count: {0}")]
    InvalidWorkerCount(String),
}

/// Errors that can occur during runtime operations.
#[derive(Debug, Error)]
pub enum RuntimeError {
    /// Error sending a message through the channel.
    #[error("Channel send error: {0}")]
    ChannelSend(String),

    /// Error receiving a message from the channel.
    #[error("Channel receive error: {0}")]
    ChannelRecv(String),

    /// Python-related error.
    #[error("Python error: {0}")]
    Python(String),

    /// Thread-related error.
    #[error("Thread error: {0}")]
    Thread(String),

    /// Configuration error.
    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),
}
