//! RustyWrapper - Flask-style Python routing for Rust/Axum applications.
//!
//! RustyWrapper provides a seamless way to integrate Python handlers into your
//! Axum web server. It uses a dedicated thread pool for Python execution and
//! supports both in-thread handlers and CPU-bound work via ProcessPoolExecutor.
//!
//! # Quick Start
//!
//! ```ignore
//! use axum::{routing::any, Router};
//! use rustywrapper::prelude::*;
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Initialize Python
//!     pyo3::Python::initialize();
//!
//!     // Configure the Python runtime
//!     let config = RustyWrapperConfig::builder()
//!         .python_dir("./python")
//!         .module("endpoints")
//!         .module("pool_handlers")
//!         .pool_workers(4)
//!         .dispatch_workers(4)
//!         .build()?;
//!
//!     let runtime = Arc::new(PythonRuntime::with_config(config)?);
//!
//!     let app = Router::new()
//!         .route("/python/{*path}", any(handle_python_request))
//!         .with_state(runtime);
//!
//!     let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
//!     axum::serve(listener, app).await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! # Architecture
//!
//! RustyWrapper uses a dedicated thread pool for Python execution:
//!
//! - **Rust endpoints** run on the Tokio async event loop
//! - **Python endpoints** run on dedicated OS threads with channel communication
//! - The GIL is held per-request, not blocking the async runtime
//!
//! # Python Handler Example
//!
//! In your Python handlers directory, create files like `endpoints.py`:
//!
//! ```python
//! from rustywrapper import route, Request
//!
//! @route('/python/hello', methods=['GET'])
//! def hello(request: Request) -> dict:
//!     return {"message": "Hello from Python"}
//!
//! @route('/python/process', methods=['POST'], use_process_pool=True)
//! def process(request: Request, pool) -> dict:
//!     # CPU-bound work can use the process pool
//!     future = pool.submit(heavy_computation, request.body)
//!     return {"result": future.result()}
//! ```

mod config;
mod dispatcher;
mod error;
mod python_runtime;

pub use config::{RustyWrapperConfig, RustyWrapperConfigBuilder};
pub use dispatcher::handle_python_request;
pub use error::{ConfigError, RuntimeError};
pub use python_runtime::{DispatchResult, PythonRuntime};

/// Prelude module for convenient imports.
///
/// Use `use rustywrapper::prelude::*;` to import all common types.
pub mod prelude {
    pub use crate::config::{RustyWrapperConfig, RustyWrapperConfigBuilder};
    pub use crate::dispatcher::handle_python_request;
    pub use crate::error::{ConfigError, RuntimeError};
    pub use crate::python_runtime::{DispatchResult, PythonRuntime};
}
