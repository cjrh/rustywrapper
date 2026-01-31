use crate::config::RustyWrapperConfig;
use crate::error::RuntimeError;
use crossbeam_channel::{Receiver, Sender, bounded};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyModule};
use std::ffi::CString;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tokio::sync::oneshot;

/// Shared Python state accessible by all worker threads.
/// Contains `Py<PyAny>` handles which are thread-safe and can be used
/// with `Python::attach()` from any thread.
struct SharedPythonState {
    dispatch_fn: Py<PyAny>,
    executor: Py<PyAny>,
}

// Safety: Py<PyAny> is Send + Sync, and we only access the underlying
// Python objects while holding the GIL via Python::attach()
unsafe impl Send for SharedPythonState {}
unsafe impl Sync for SharedPythonState {}

/// The Python runtime that manages Python execution threads and message passing.
pub struct PythonRuntime {
    sender: Sender<RuntimeMessage>,
    workers: Vec<JoinHandle<()>>,
    state: Arc<SharedPythonState>,
    worker_count: usize,
}

enum RuntimeMessage {
    Dispatch {
        method: String,
        path: String,
        request_data: serde_json::Value,
        response_tx: oneshot::Sender<DispatchResult>,
    },
    Shutdown,
}

/// Result of dispatching a request to Python.
#[derive(Debug)]
pub struct DispatchResult {
    /// Whether the request was handled successfully.
    pub success: bool,
    /// HTTP status code.
    pub code: u16,
    /// Response data (if successful).
    pub data: Option<serde_json::Value>,
    /// Error message (if failed).
    pub error: Option<String>,
}

impl PythonRuntime {
    /// Create a new Python runtime with the given configuration.
    ///
    /// This is the preferred way to create a `PythonRuntime`. It allows full
    /// control over the Python environment through the configuration.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use rustywrapper::{RustyWrapperConfig, PythonRuntime};
    ///
    /// let config = RustyWrapperConfig::builder()
    ///     .python_dir("./python")
    ///     .module("endpoints")
    ///     .pool_workers(4)
    ///     .build()?;
    ///
    /// let runtime = PythonRuntime::with_config(config)?;
    /// ```
    pub fn with_config(config: RustyWrapperConfig) -> Result<Self, RuntimeError> {
        let python_dir_str = config
            .python_dir
            .to_str()
            .ok_or_else(|| RuntimeError::Python("Python path contains invalid UTF-8".to_string()))?
            .to_string();

        tracing::info!("Python module path: {}", python_dir_str);

        // Setup signal handler if not disabled
        if !config.disable_signal_handler {
            Python::attach(|py| {
                if let Err(e) = Self::setup_signal_handler(py) {
                    tracing::warn!("Failed to setup Python signal handler: {}", e);
                }
            });
        }

        // Initialize Python and create shared state (must be done with GIL)
        let state = Python::attach(|py| {
            Self::initialize_python(
                py,
                config.pool_workers,
                &config.modules,
                &python_dir_str,
            )
        })
        .map_err(|e| RuntimeError::Python(e.to_string()))?;

        let state = Arc::new(state);

        // Create bounded channel for message passing
        // Buffer size allows some queuing without blocking senders
        let (sender, receiver) = bounded::<RuntimeMessage>(config.dispatch_workers * 2);

        // Spawn worker threads
        let mut workers = Vec::with_capacity(config.dispatch_workers);
        for worker_id in 0..config.dispatch_workers {
            let receiver = receiver.clone();
            let state = Arc::clone(&state);

            let handle = thread::Builder::new()
                .name(format!("python-worker-{}", worker_id))
                .spawn(move || {
                    Self::worker_loop(worker_id, receiver, state);
                })
                .map_err(|e| RuntimeError::Thread(e.to_string()))?;

            workers.push(handle);
        }

        tracing::info!(
            "Started {} Python dispatch workers",
            config.dispatch_workers
        );

        Ok(Self {
            sender,
            workers,
            state,
            worker_count: config.dispatch_workers,
        })
    }

    /// Create a new Python runtime with default configuration.
    ///
    /// This constructor looks for Python modules in `./python` relative to the
    /// current working directory.
    ///
    /// # Deprecated
    ///
    /// This method is deprecated in favor of [`PythonRuntime::with_config`],
    /// which provides more control and better error handling.
    #[deprecated(
        since = "0.2.0",
        note = "Use PythonRuntime::with_config() for better configuration control"
    )]
    pub fn new(
        pool_workers: usize,
        dispatch_workers: usize,
        python_modules: &[&str],
    ) -> Result<Self, RuntimeError> {
        let config = RustyWrapperConfig::builder()
            .python_dir("python")
            .modules(python_modules.iter().map(|s| s.to_string()))
            .pool_workers(pool_workers)
            .dispatch_workers(dispatch_workers)
            .build()
            .map_err(RuntimeError::Config)?;

        Self::with_config(config)
    }

    /// Setup Python's signal handler to use SIG_DFL for SIGINT.
    ///
    /// This allows Rust to handle Ctrl+C for graceful shutdown.
    fn setup_signal_handler(py: Python<'_>) -> PyResult<()> {
        let signal_mod = py.import("signal")?;
        let sigint = signal_mod.getattr("SIGINT")?;
        let sig_dfl = signal_mod.getattr("SIG_DFL")?;
        signal_mod.call_method1("signal", (sigint, sig_dfl))?;
        Ok(())
    }

    /// Initialize Python environment and create shared state.
    /// Must be called with the GIL held.
    fn initialize_python(
        py: Python<'_>,
        pool_workers: usize,
        python_modules: &[String],
        python_dir: &str,
    ) -> PyResult<SharedPythonState> {
        // Add python/ directory to sys.path using absolute path
        let sys = py.import("sys")?;
        let path = sys.getattr("path")?;
        path.call_method1("insert", (0, python_dir))?;

        // Embed the rustywrapper framework at compile time
        let rustywrapper_code = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/python/rustywrapper.py"
        ));

        // Load as a module (registers in sys.modules automatically)
        let rustywrapper = PyModule::from_code(
            py,
            CString::new(rustywrapper_code)
                .expect("rustywrapper.py contains null byte")
                .as_c_str(),
            c"rustywrapper.py",
            c"rustywrapper",
        )?;

        // Import user modules (which registers routes via @route decorators)
        for module_name in python_modules {
            match py.import(module_name.as_str()) {
                Ok(_) => tracing::info!("Loaded Python module: {}", module_name),
                Err(e) => {
                    tracing::error!("Failed to import {}: {}", module_name, e);
                    e.print(py);
                }
            }
        }

        // Create ProcessPoolExecutor
        let pool_workers_module = py.import("pool_workers").ok();
        let executor = Self::create_pool(py, pool_workers, pool_workers_module.as_ref())?;

        // Log registered routes
        Self::log_routes(&rustywrapper);

        // Get dispatch function
        let dispatch_fn = rustywrapper.getattr("dispatch")?;

        // Convert to Py<PyAny> for thread-safe storage
        Ok(SharedPythonState {
            dispatch_fn: dispatch_fn.unbind(),
            executor: executor.unbind(),
        })
    }

    /// Worker loop that processes messages from the channel.
    /// Each worker acquires the GIL per request, allowing interleaving
    /// when Python releases the GIL (e.g., during future.result() waits).
    fn worker_loop(
        worker_id: usize,
        receiver: Receiver<RuntimeMessage>,
        state: Arc<SharedPythonState>,
    ) {
        tracing::debug!("Python worker {} started", worker_id);

        loop {
            match receiver.recv_timeout(Duration::from_millis(100)) {
                Ok(RuntimeMessage::Dispatch {
                    method,
                    path,
                    request_data,
                    response_tx,
                }) => {
                    // Acquire GIL and process the request
                    let result = Python::attach(|py| {
                        let dispatch_fn = state.dispatch_fn.bind(py);
                        let executor = state.executor.bind(py);
                        Self::handle_dispatch(py, &dispatch_fn, &executor, method, path, request_data)
                    });
                    let _ = response_tx.send(result);
                }
                Ok(RuntimeMessage::Shutdown) => {
                    tracing::debug!("Python worker {} received shutdown signal", worker_id);
                    break;
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    // No message, continue waiting
                    continue;
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    tracing::debug!("Python worker {} channel disconnected", worker_id);
                    break;
                }
            }
        }

        tracing::debug!("Python worker {} exiting", worker_id);
    }

    fn create_pool<'py>(
        py: Python<'py>,
        max_workers: usize,
        pool_workers_module: Option<&Bound<'py, PyModule>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let concurrent_futures = py.import("concurrent.futures")?;
        let kwargs = PyDict::new(py);
        kwargs.set_item("max_workers", max_workers)?;

        // Set initializer if pool_workers module is available
        if let Some(workers) = pool_workers_module {
            if let Ok(initializer) = workers.getattr("init_worker") {
                kwargs.set_item("initializer", initializer)?;
            }
        }

        let executor = concurrent_futures
            .getattr("ProcessPoolExecutor")?
            .call((), Some(&kwargs))?;

        tracing::info!(
            "Created ProcessPoolExecutor with {} workers",
            max_workers
        );

        Ok(executor)
    }

    fn shutdown_pool(py: Python<'_>, executor: &Bound<'_, PyAny>) {
        let kwargs = PyDict::new(py);
        let _ = kwargs.set_item("wait", true);
        let _ = kwargs.set_item("cancel_futures", true);
        match executor.call_method("shutdown", (), Some(&kwargs)) {
            Ok(_) => tracing::info!("ProcessPoolExecutor shut down successfully"),
            Err(e) => {
                if !e.is_instance_of::<pyo3::exceptions::PyKeyboardInterrupt>(py) {
                    tracing::error!("Error shutting down pool: {}", e);
                }
            }
        }
    }

    fn log_routes(rustywrapper: &Bound<'_, PyModule>) {
        if let Ok(list_routes) = rustywrapper.getattr("list_routes") {
            if let Ok(routes) = list_routes.call0() {
                if let Ok(routes_list) = routes.extract::<Bound<'_, PyList>>() {
                    tracing::info!("Registered Python routes:");
                    for route in routes_list.iter() {
                        if let (Ok(method), Ok(path), Ok(handler), Ok(pool)) = (
                            route.get_item("method"),
                            route.get_item("path"),
                            route.get_item("handler"),
                            route.get_item("use_process_pool"),
                        ) {
                            let pool_marker = if pool.is_truthy().unwrap_or(false) {
                                " [pool]"
                            } else {
                                ""
                            };
                            tracing::info!("  {} {} -> {}(){}", method, path, handler, pool_marker);
                        }
                    }
                }
            }
        }
    }

    fn handle_dispatch(
        py: Python<'_>,
        dispatch_fn: &Bound<'_, PyAny>,
        executor: &Bound<'_, PyAny>,
        method: String,
        path: String,
        request_data: serde_json::Value,
    ) -> DispatchResult {
        // Convert request_data to Python dict
        let request_dict = match Self::json_to_pydict(py, &request_data) {
            Ok(d) => d,
            Err(e) => {
                return DispatchResult {
                    success: false,
                    code: 500,
                    data: None,
                    error: Some(format!("Failed to convert request data: {}", e)),
                }
            }
        };

        // Call dispatch(method, path, request_data, pool)
        let result = dispatch_fn.call1((&method, &path, request_dict, executor));

        match result {
            Ok(response) => Self::parse_dispatch_response(py, &response),
            Err(e) => {
                let error_msg = e.to_string();
                e.print(py);
                DispatchResult {
                    success: false,
                    code: 500,
                    data: None,
                    error: Some(error_msg),
                }
            }
        }
    }

    fn parse_dispatch_response(py: Python<'_>, response: &Bound<'_, PyAny>) -> DispatchResult {
        let success = response
            .get_item("success")
            .ok()
            .and_then(|v| v.extract::<bool>().ok())
            .unwrap_or(false);

        let code = response
            .get_item("code")
            .ok()
            .and_then(|v| v.extract::<u16>().ok())
            .unwrap_or(if success { 200 } else { 500 });

        let data = if success {
            response
                .get_item("data")
                .ok()
                .and_then(|v| Self::pyany_to_json(py, &v).ok())
        } else {
            None
        };

        let error = if !success {
            response
                .get_item("error")
                .ok()
                .and_then(|v| v.extract::<String>().ok())
        } else {
            None
        };

        DispatchResult {
            success,
            code,
            data,
            error,
        }
    }

    fn json_to_pydict<'py>(
        py: Python<'py>,
        value: &serde_json::Value,
    ) -> PyResult<Bound<'py, PyDict>> {
        let json_module = py.import("json")?;
        let json_str = serde_json::to_string(value)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(e.to_string()))?;
        let py_obj = json_module.call_method1("loads", (json_str,))?;
        py_obj
            .extract::<Bound<'py, PyDict>>()
            .map_err(|_| PyErr::new::<pyo3::exceptions::PyTypeError, _>("Expected dict"))
    }

    fn pyany_to_json(_py: Python<'_>, value: &Bound<'_, PyAny>) -> Result<serde_json::Value, String> {
        let json_module = value.py().import("json").map_err(|e| e.to_string())?;
        let json_str = json_module
            .call_method1("dumps", (value,))
            .map_err(|e| e.to_string())?;
        let rust_str: String = json_str.extract::<String>().map_err(|e| e.to_string())?;
        serde_json::from_str(&rust_str).map_err(|e| e.to_string())
    }

    /// Dispatch a request to Python.
    ///
    /// This sends the request to a worker thread which will execute the
    /// appropriate Python handler.
    pub async fn dispatch(
        &self,
        method: &str,
        path: &str,
        request_data: serde_json::Value,
    ) -> Result<DispatchResult, RuntimeError> {
        let (tx, rx) = oneshot::channel();

        self.sender
            .send(RuntimeMessage::Dispatch {
                method: method.to_string(),
                path: path.to_string(),
                request_data,
                response_tx: tx,
            })
            .map_err(|e| RuntimeError::ChannelSend(e.to_string()))?;

        rx.await
            .map_err(|e| RuntimeError::ChannelRecv(e.to_string()))
    }

    /// Shutdown the Python runtime gracefully.
    ///
    /// This sends shutdown signals to all worker threads and shuts down
    /// the ProcessPoolExecutor.
    pub fn shutdown(&self) -> Result<(), RuntimeError> {
        tracing::info!("Initiating Python runtime shutdown...");

        // Send shutdown message to each worker
        for i in 0..self.worker_count {
            if let Err(e) = self.sender.send(RuntimeMessage::Shutdown) {
                tracing::warn!("Failed to send shutdown to worker {}: {}", i, e);
            }
        }

        // Shutdown the ProcessPoolExecutor with GIL
        Python::attach(|py| {
            Self::shutdown_pool(py, &self.state.executor.bind(py));
        });

        Ok(())
    }
}

impl Drop for PythonRuntime {
    fn drop(&mut self) {
        // Send shutdown to all workers
        for _ in 0..self.worker_count {
            let _ = self.sender.send(RuntimeMessage::Shutdown);
        }

        // Join all worker threads
        for handle in self.workers.drain(..) {
            if let Err(e) = handle.join() {
                tracing::error!("Worker thread panicked: {:?}", e);
            }
        }
    }
}
