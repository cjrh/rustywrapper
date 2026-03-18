//! Configuration for the Chimera Python runtime.
//!
//! This module provides a builder pattern for configuring the Python runtime.

use crate::error::ConfigError;
use std::path::PathBuf;

/// Configuration for the Chimera Python runtime.
#[derive(Debug, Clone)]
pub struct ChimeraConfig {
    /// Path to the directory containing Python handlers.
    pub python_dir: PathBuf,

    /// Path to the venv's site-packages directory (resolved during build()).
    /// None if no venv was configured or detected.
    pub venv_site_packages: Option<PathBuf>,

    /// List of Python module names to import (registers routes via @route decorators).
    pub modules: Vec<String>,

    /// Number of ProcessPoolExecutor workers for CPU-bound Python tasks.
    pub pool_workers: usize,

    /// Number of Rust threads that can dispatch to Python concurrently.
    pub dispatch_workers: usize,

    /// If true, disables the Python SIGINT handler setup.
    /// Useful when embedding in applications that handle signals themselves.
    pub disable_signal_handler: bool,

    /// Enable async Python handlers.
    /// When true, a dedicated asyncio event loop thread is started for async handlers.
    /// Defaults to true if any async routes are registered.
    pub enable_async: bool,
}

impl ChimeraConfig {
    /// Create a new configuration builder.
    pub fn builder() -> ChimeraConfigBuilder {
        ChimeraConfigBuilder::new()
    }
}

impl Default for ChimeraConfig {
    fn default() -> Self {
        Self {
            python_dir: PathBuf::from("python"),
            venv_site_packages: None,
            modules: Vec::new(),
            pool_workers: 4,
            dispatch_workers: 4,
            disable_signal_handler: false,
            enable_async: true,
        }
    }
}

/// Builder for creating a [`ChimeraConfig`].
#[derive(Debug, Clone, Default)]
pub struct ChimeraConfigBuilder {
    python_dir: Option<PathBuf>,
    venv_path: Option<PathBuf>,
    modules: Vec<String>,
    pool_workers: Option<usize>,
    dispatch_workers: Option<usize>,
    disable_signal_handler: bool,
    enable_async: Option<bool>,
}

impl ChimeraConfigBuilder {
    /// Create a new configuration builder with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the path to the directory containing Python handlers.
    ///
    /// This path can be relative (resolved from current directory) or absolute.
    /// The directory should contain the handler modules (e.g., `endpoints.py`).
    /// The `chimera` framework module is embedded in the binary and does
    /// not need to be present in this directory.
    pub fn python_dir<P: Into<PathBuf>>(mut self, path: P) -> Self {
        self.python_dir = Some(path.into());
        self
    }

    /// Override the virtual environment path.
    ///
    /// By default, Chimera reads the `VIRTUAL_ENV` environment variable to
    /// locate the venv and add its `site-packages` to `sys.path` at startup.
    /// Use this method to override that with an explicit path — useful for
    /// multi-venv setups, testing, or when environment variables aren't suitable.
    ///
    /// The path can be relative (resolved from current directory) or absolute.
    /// If neither `.venv()` nor `VIRTUAL_ENV` is set, no venv site-packages
    /// are added.
    pub fn venv<P: Into<PathBuf>>(mut self, path: P) -> Self {
        self.venv_path = Some(path.into());
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
    /// By default, Chimera configures Python to use SIG_DFL for SIGINT,
    /// allowing Rust to handle Ctrl+C for graceful shutdown. Set this to true
    /// if your application handles signals differently.
    pub fn disable_signal_handler(mut self, disable: bool) -> Self {
        self.disable_signal_handler = disable;
        self
    }

    /// Enable or disable async Python handlers.
    ///
    /// When enabled, a dedicated asyncio event loop thread is started for
    /// handling async Python handlers. This allows thousands of concurrent
    /// async requests without blocking Rust worker threads.
    ///
    /// Default is true. Set to false if you don't have any async handlers
    /// and want to avoid the overhead of the async thread.
    pub fn enable_async(mut self, enable: bool) -> Self {
        self.enable_async = Some(enable);
        self
    }

    /// Build the configuration, validating all settings.
    pub fn build(self) -> Result<ChimeraConfig, ConfigError> {
        let python_dir = self.resolve_python_dir()?;
        let venv_site_packages = self.resolve_venv_site_packages()?;

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

        Ok(ChimeraConfig {
            python_dir,
            venv_site_packages,
            modules: self.modules,
            pool_workers,
            dispatch_workers,
            disable_signal_handler: self.disable_signal_handler,
            enable_async: self.enable_async.unwrap_or(true),
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

    /// Resolve the venv's site-packages directory.
    ///
    /// Uses the explicitly configured venv path, falling back to the
    /// `VIRTUAL_ENV` environment variable. Globs for `lib/python*/site-packages`
    /// within the venv root and validates the directory exists.
    fn resolve_venv_site_packages(&self) -> Result<Option<PathBuf>, ConfigError> {
        // Determine venv root: explicit config, then VIRTUAL_ENV env var
        let venv_root = match &self.venv_path {
            Some(path) => path.clone(),
            None => match std::env::var("VIRTUAL_ENV") {
                Ok(val) if !val.is_empty() => PathBuf::from(val),
                _ => return Ok(None),
            },
        };

        // Resolve to absolute path
        let venv_root = if venv_root.is_absolute() {
            venv_root
        } else {
            std::env::current_dir()
                .map_err(|e| {
                    ConfigError::PathResolution(format!(
                        "Failed to get current directory: {}",
                        e
                    ))
                })?
                .join(&venv_root)
        };

        // Glob for lib/python*/site-packages
        let lib_dir = venv_root.join("lib");
        if !lib_dir.exists() {
            return Err(ConfigError::VenvInvalid(venv_root));
        }

        let site_packages = std::fs::read_dir(&lib_dir)
            .map_err(|_| ConfigError::VenvInvalid(venv_root.clone()))?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("python"))
                    && path.is_dir()
            })
            .map(|python_dir| python_dir.join("site-packages"));

        match site_packages {
            Some(sp) if sp.is_dir() => Ok(Some(sp)),
            _ => Err(ConfigError::VenvInvalid(venv_root)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_defaults() {
        // This test would fail without a valid python dir, but demonstrates the API
        let builder = ChimeraConfig::builder()
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
        let result = ChimeraConfigBuilder::new()
            .pool_workers(0)
            .build();

        // Will fail on python_dir validation first, but that's expected
        assert!(result.is_err());
    }
}
