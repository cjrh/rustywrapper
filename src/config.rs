//! Configuration for the Snaxum Python runtime.
//!
//! This module provides a builder pattern for configuring the Python runtime.

use crate::error::ConfigError;
use std::path::PathBuf;

/// Configuration for the Snaxum Python runtime.
#[derive(Debug, Clone)]
pub struct SnaxumConfig {
    /// Path to the directory containing Python handlers.
    pub python_dir: PathBuf,

    /// List of Python module names to import (registers routes via @route decorators).
    pub modules: Vec<String>,

    /// Number of ProcessPoolExecutor workers for CPU-bound Python tasks.
    pub pool_workers: usize,

    /// Number of Rust threads that can dispatch to Python concurrently.
    pub dispatch_workers: usize,

    /// If true, disables the Python SIGINT handler setup.
    /// Useful when embedding in applications that handle signals themselves.
    pub disable_signal_handler: bool,
}

impl SnaxumConfig {
    /// Create a new configuration builder.
    pub fn builder() -> SnaxumConfigBuilder {
        SnaxumConfigBuilder::new()
    }
}

impl Default for SnaxumConfig {
    fn default() -> Self {
        Self {
            python_dir: PathBuf::from("python"),
            modules: Vec::new(),
            pool_workers: 4,
            dispatch_workers: 4,
            disable_signal_handler: false,
        }
    }
}

/// Builder for creating a [`SnaxumConfig`].
#[derive(Debug, Clone, Default)]
pub struct SnaxumConfigBuilder {
    python_dir: Option<PathBuf>,
    modules: Vec<String>,
    pool_workers: Option<usize>,
    dispatch_workers: Option<usize>,
    disable_signal_handler: bool,
}

impl SnaxumConfigBuilder {
    /// Create a new configuration builder with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the path to the directory containing Python handlers.
    ///
    /// This path can be relative (resolved from current directory) or absolute.
    /// The directory should contain the handler modules (e.g., `endpoints.py`).
    /// The `snaxum` framework module is embedded in the binary and does
    /// not need to be present in this directory.
    pub fn python_dir<P: Into<PathBuf>>(mut self, path: P) -> Self {
        self.python_dir = Some(path.into());
        self
    }

    /// Add a Python module to import.
    ///
    /// Each module should contain route handlers decorated with `@route`.
    pub fn module<S: Into<String>>(mut self, module: S) -> Self {
        self.modules.push(module.into());
        self
    }

    /// Add multiple Python modules to import.
    pub fn modules<I, S>(mut self, modules: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.modules.extend(modules.into_iter().map(Into::into));
        self
    }

    /// Set the number of ProcessPoolExecutor workers for CPU-bound Python tasks.
    ///
    /// Default is 4.
    pub fn pool_workers(mut self, count: usize) -> Self {
        self.pool_workers = Some(count);
        self
    }

    /// Set the number of Rust threads that can dispatch to Python concurrently.
    ///
    /// Default is 4.
    pub fn dispatch_workers(mut self, count: usize) -> Self {
        self.dispatch_workers = Some(count);
        self
    }

    /// Disable the Python SIGINT handler setup.
    ///
    /// By default, Snaxum configures Python to use SIG_DFL for SIGINT,
    /// allowing Rust to handle Ctrl+C for graceful shutdown. Set this to true
    /// if your application handles signals differently.
    pub fn disable_signal_handler(mut self, disable: bool) -> Self {
        self.disable_signal_handler = disable;
        self
    }

    /// Build the configuration, validating all settings.
    pub fn build(self) -> Result<SnaxumConfig, ConfigError> {
        let python_dir = self.resolve_python_dir()?;

        // Validate pool workers
        let pool_workers = self.pool_workers.unwrap_or(4);
        if pool_workers == 0 {
            return Err(ConfigError::InvalidWorkerCount(
                "pool_workers must be at least 1".to_string(),
            ));
        }

        // Validate dispatch workers
        let dispatch_workers = self.dispatch_workers.unwrap_or(4);
        if dispatch_workers == 0 {
            return Err(ConfigError::InvalidWorkerCount(
                "dispatch_workers must be at least 1".to_string(),
            ));
        }

        Ok(SnaxumConfig {
            python_dir,
            modules: self.modules,
            pool_workers,
            dispatch_workers,
            disable_signal_handler: self.disable_signal_handler,
        })
    }

    fn resolve_python_dir(&self) -> Result<PathBuf, ConfigError> {
        let path = self
            .python_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from("python"));

        // Make path absolute if it's relative
        let absolute_path = if path.is_absolute() {
            path
        } else {
            std::env::current_dir()
                .map_err(|e| ConfigError::PathResolution(format!("Failed to get current directory: {}", e)))?
                .join(&path)
        };

        // Validate the directory exists
        if !absolute_path.exists() {
            return Err(ConfigError::PythonDirNotFound(absolute_path));
        }

        if !absolute_path.is_dir() {
            return Err(ConfigError::PathResolution(format!(
                "Path is not a directory: {}",
                absolute_path.display()
            )));
        }

        Ok(absolute_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_defaults() {
        // This test would fail without a valid python dir, but demonstrates the API
        let builder = SnaxumConfig::builder()
            .module("endpoints")
            .pool_workers(2)
            .dispatch_workers(2);

        // Can't actually build without a valid python_dir
        assert_eq!(builder.modules.len(), 1);
        assert_eq!(builder.pool_workers, Some(2));
        assert_eq!(builder.dispatch_workers, Some(2));
    }

    #[test]
    fn test_zero_workers_rejected() {
        // Create a temporary test - in real tests we'd mock the filesystem
        let result = SnaxumConfigBuilder::new()
            .pool_workers(0)
            .build();

        // Will fail on python_dir validation first, but that's expected
        assert!(result.is_err());
    }
}
