use crate::config::SnaxumConfig;
use crate::error::RuntimeError;
use crossbeam_channel::{Receiver, Sender, bounded};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyModule};
use std::collections::HashMap;
use std::ffi::CString;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tokio::sync::oneshot;

/// Shared Python state accessible by all worker threads.
/// Contains `Py<PyAny>` handles which are thread-safe and can be used
/// with `Python::attach()` from any thread.
struct SharedPythonState {
    dispatch_fn: Py<PyAny>,
    get_route_info_fn: Py<PyAny>,
    executor: Py<PyAny>,
}

/// Shared state for the async Python runtime.
struct AsyncPythonState {
    submit_async_fn: Py<PyAny>,
    poll_responses_fn: Py<PyAny>,
    shutdown_async_fn: Py<PyAny>,
}

// Safety: Py<PyAny> is Send + Sync, and we only access the underlying
// Python objects while holding the GIL via Python::attach()
unsafe impl Send for SharedPythonState {}
unsafe impl Sync for SharedPythonState {}
unsafe impl Send for AsyncPythonState {}
unsafe impl Sync for AsyncPythonState {}

/// The Python runtime that manages Python execution threads and message passing.
pub struct PythonRuntime {
    // Sync dispatch
    sender: Sender<RuntimeMessage>,
    workers: Vec<JoinHandle<()>>,
    state: Arc<SharedPythonState>,
    worker_count: usize,

    // Async dispatch
    async_enabled: bool,
    async_sender: Option<Sender<AsyncRuntimeMessage>>,
    async_worker: Option<JoinHandle<()>>,
    async_state: Option<Arc<AsyncPythonState>>,
    async_pending: Arc<Mutex<HashMap<u64, oneshot::Sender<DispatchResult>>>>,
    async_request_id: AtomicU64,
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

enum AsyncRuntimeMessage {
    Dispatch {
        request_id: u64,
        method: String,
        path: String,
        request_data: serde_json::Value,
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
    /// use snaxum::{SnaxumConfig, PythonRuntime};
    ///
    /// let config = SnaxumConfig::builder()
    ///     .python_dir("./python")
    ///     .module("endpoints")
    ///     .pool_workers(4)
    ///     .build()?;
    ///
    /// let runtime = PythonRuntime::with_config(config)?;
    /// ```
    pub fn with_config(config: SnaxumConfig) -> Result<Self, RuntimeError> {
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

        // Initialize async runtime if enabled
        let async_pending = Arc::new(Mutex::new(HashMap::new()));
        let (async_enabled, async_sender, async_worker, async_state) = if config.enable_async {
            let (async_state, async_sender, async_worker) =
                Self::initialize_async_runtime(&state, Arc::clone(&async_pending))?;
            (true, Some(async_sender), Some(async_worker), Some(async_state))
        } else {
            (false, None, None, None)
        };

        Ok(Self {
            sender,
            workers,
            state,
            worker_count: config.dispatch_workers,
            async_enabled,
            async_sender,
            async_worker,
            async_state,
            async_pending,
            async_request_id: AtomicU64::new(1),
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
        let config = SnaxumConfig::builder()
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

        // Embed the snaxum framework at compile time
        let snaxum_code = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/python/snaxum.py"
        ));

        // Load as a module (registers in sys.modules automatically)
        let snaxum = PyModule::from_code(
            py,
            CString::new(snaxum_code)
                .expect("snaxum.py contains null byte")
                .as_c_str(),
            c"snaxum.py",
            c"snaxum",
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
        Self::log_routes(&snaxum);

        // Get dispatch and route info functions
        let dispatch_fn = snaxum.getattr("dispatch")?;
        let get_route_info_fn = snaxum.getattr("get_route_info")?;

        // Convert to Py<PyAny> for thread-safe storage
        Ok(SharedPythonState {
            dispatch_fn: dispatch_fn.unbind(),
            get_route_info_fn: get_route_info_fn.unbind(),
            executor: executor.unbind(),
        })
    }

    /// Initialize the async Python runtime.
    ///
    /// This starts the async_runtime.py module and returns the state needed
    /// for async dispatch.
    fn initialize_async_runtime(
        state: &Arc<SharedPythonState>,
        pending: Arc<Mutex<HashMap<u64, oneshot::Sender<DispatchResult>>>>,
    ) -> Result<(Arc<AsyncPythonState>, Sender<AsyncRuntimeMessage>, JoinHandle<()>), RuntimeError>
    {
        // Embed the async_runtime module at compile time
        let async_runtime_code = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/python/async_runtime.py"
        ));

        // Initialize async runtime in Python
        let async_state = Python::attach(|py| {
            // Load async_runtime module
            let async_runtime = PyModule::from_code(
                py,
                CString::new(async_runtime_code)
                    .expect("async_runtime.py contains null byte")
                    .as_c_str(),
                c"async_runtime.py",
                c"async_runtime",
            )?;

            // Start the async thread with the pool
            let start_fn = async_runtime.getattr("start_async_runtime")?;
            let executor = state.executor.bind(py);
            start_fn.call1((executor,))?;

            tracing::info!("Started Python async runtime thread");

            // Get async functions
            let submit_async_fn = async_runtime.getattr("submit_async")?;
            let poll_responses_fn = async_runtime.getattr("poll_responses")?;
            let shutdown_async_fn = async_runtime.getattr("shutdown_async_runtime")?;

            Ok::<_, PyErr>(AsyncPythonState {
                submit_async_fn: submit_async_fn.unbind(),
                poll_responses_fn: poll_responses_fn.unbind(),
                shutdown_async_fn: shutdown_async_fn.unbind(),
            })
        })
        .map_err(|e| RuntimeError::Python(e.to_string()))?;

        // Create async channel
        let (async_sender, async_receiver) = bounded::<AsyncRuntimeMessage>(1000);

        // Spawn async worker thread - both the worker and the runtime struct share the state
        let async_state_arc = Arc::new(async_state);
        let async_state_for_worker = Arc::clone(&async_state_arc);

        let async_worker = thread::Builder::new()
            .name("python-async-worker".to_string())
            .spawn(move || {
                Self::async_worker_loop(async_receiver, async_state_for_worker, pending);
            })
            .map_err(|e| RuntimeError::Thread(e.to_string()))?;

        Ok((async_state_arc, async_sender, async_worker))
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
                    #[allow(clippy::needless_borrow)]
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

    /// Async worker loop that polls Python for completed async responses and
    /// submits new async requests.
    fn async_worker_loop(
        receiver: Receiver<AsyncRuntimeMessage>,
        state: Arc<AsyncPythonState>,
        pending: Arc<Mutex<HashMap<u64, oneshot::Sender<DispatchResult>>>>,
    ) {
        tracing::debug!("Python async worker started");

        loop {
            // 1. Poll for completed responses (brief GIL acquisition)
            Python::attach(|py| {
                let poll_fn = state.poll_responses_fn.bind(py);
                match poll_fn.call0() {
                    Ok(responses) => {
                        // responses is a list of (request_id, response_dict) tuples
                        if let Ok(list) = responses.extract::<Vec<(u64, Bound<'_, PyAny>)>>() {
                            for (request_id, response) in list {
                                let result = Self::parse_dispatch_response(py, &response);
                                if let Some(sender) = pending.lock().unwrap().remove(&request_id) {
                                    let _ = sender.send(result);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Error polling async responses: {}", e);
                    }
                }
            });

            // 2. Check for new requests or shutdown
            match receiver.recv_timeout(Duration::from_millis(10)) {
                Ok(AsyncRuntimeMessage::Dispatch {
                    request_id,
                    method,
                    path,
                    request_data,
                }) => {
                    // Submit to Python async queue
                    Python::attach(|py| {
                        let submit_fn = state.submit_async_fn.bind(py);
                        let data_dict = match Self::json_to_pydict(py, &request_data) {
                            Ok(d) => d,
                            Err(e) => {
                                tracing::error!("Failed to convert request data: {}", e);
                                // Send error response
                                if let Some(sender) = pending.lock().unwrap().remove(&request_id) {
                                    let _ = sender.send(DispatchResult {
                                        success: false,
                                        code: 500,
                                        data: None,
                                        error: Some(format!("Failed to convert request data: {}", e)),
                                    });
                                }
                                return;
                            }
                        };

                        if let Err(e) =
                            submit_fn.call1((request_id, &method, &path, data_dict))
                        {
                            tracing::error!("Failed to submit async request: {}", e);
                            if let Some(sender) = pending.lock().unwrap().remove(&request_id) {
                                let _ = sender.send(DispatchResult {
                                    success: false,
                                    code: 500,
                                    data: None,
                                    error: Some(format!("Failed to submit async request: {}", e)),
                                });
                            }
                        }
                    });
                }
                Ok(AsyncRuntimeMessage::Shutdown) => {
                    tracing::debug!("Python async worker received shutdown signal");
                    break;
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    // No message, continue polling
                    continue;
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    tracing::debug!("Python async worker channel disconnected");
                    break;
                }
            }
        }

        tracing::debug!("Python async worker exiting");
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

    fn log_routes(snaxum: &Bound<'_, PyModule>) {
        if let Ok(list_routes) = snaxum.getattr("list_routes") {
            if let Ok(routes) = list_routes.call0() {
                if let Ok(routes_list) = routes.extract::<Bound<'_, PyList>>() {
                    tracing::info!("Registered Python routes:");
                    for route in routes_list.iter() {
                        if let (Ok(method), Ok(path), Ok(handler), Ok(pool), Ok(is_async)) = (
                            route.get_item("method"),
                            route.get_item("path"),
                            route.get_item("handler"),
                            route.get_item("use_process_pool"),
                            route.get_item("is_async"),
                        ) {
                            let pool_marker = if pool.is_truthy().unwrap_or(false) {
                                " [pool]"
                            } else {
                                ""
                            };
                            let async_marker = if is_async.is_truthy().unwrap_or(false) {
                                " [async]"
                            } else {
                                ""
                            };
                            tracing::info!(
                                "  {} {} -> {}(){}{}",
                                method, path, handler, pool_marker, async_marker
                            );
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

    /// Dispatch a sync request to Python.
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

    /// Dispatch an async request to Python.
    ///
    /// This sends the request to the async Python thread which runs an asyncio
    /// event loop. The request will be handled concurrently with other async
    /// requests without blocking worker threads.
    ///
    /// Returns an error if async is not enabled in the runtime configuration.
    pub async fn dispatch_async(
        &self,
        method: &str,
        path: &str,
        request_data: serde_json::Value,
    ) -> Result<DispatchResult, RuntimeError> {
        if !self.async_enabled {
            return Err(RuntimeError::AsyncNotEnabled);
        }

        let async_sender = self
            .async_sender
            .as_ref()
            .ok_or(RuntimeError::AsyncNotEnabled)?;

        let request_id = self.async_request_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();

        // Store the response sender
        self.async_pending.lock().unwrap().insert(request_id, tx);

        // Send to async channel
        async_sender
            .send(AsyncRuntimeMessage::Dispatch {
                request_id,
                method: method.to_string(),
                path: path.to_string(),
                request_data,
            })
            .map_err(|e: crossbeam_channel::SendError<AsyncRuntimeMessage>| {
                RuntimeError::ChannelSend(e.to_string())
            })?;

        rx.await
            .map_err(|e| RuntimeError::ChannelRecv(e.to_string()))
    }

    /// Check if a route is async.
    ///
    /// Returns Some(true) if the route is async, Some(false) if sync,
    /// or None if the route doesn't exist.
    pub fn is_route_async(&self, method: &str, path: &str) -> Option<bool> {
        Python::attach(|py| {
            let get_route_info = self.state.get_route_info_fn.bind(py);
            match get_route_info.call1((method, path)) {
                Ok(result) => {
                    if result.is_none() {
                        None
                    } else {
                        result
                            .get_item("is_async")
                            .ok()
                            .and_then(|v| v.extract::<bool>().ok())
                    }
                }
                Err(_) => None,
            }
        })
    }

    /// Check if async dispatch is enabled.
    pub fn is_async_enabled(&self) -> bool {
        self.async_enabled
    }

    /// Shutdown the Python runtime gracefully.
    ///
    /// This sends shutdown signals to all worker threads and shuts down
    /// the ProcessPoolExecutor.
    pub fn shutdown(&self) -> Result<(), RuntimeError> {
        tracing::info!("Initiating Python runtime shutdown...");

        // Send shutdown message to each sync worker
        for i in 0..self.worker_count {
            if let Err(e) = self.sender.send(RuntimeMessage::Shutdown) {
                tracing::warn!("Failed to send shutdown to worker {}: {}", i, e);
            }
        }

        // Send shutdown to async worker if enabled
        if let Some(ref async_sender) = self.async_sender {
            if let Err(e) = async_sender.send(AsyncRuntimeMessage::Shutdown) {
                tracing::warn!("Failed to send shutdown to async worker: {}", e);
            }
        }

        // Shutdown the async Python thread
        if let Some(ref async_state) = self.async_state {
            Python::attach(|py| {
                let shutdown_fn = async_state.shutdown_async_fn.bind(py);
                if let Err(e) = shutdown_fn.call0() {
                    tracing::warn!("Failed to shutdown async Python runtime: {}", e);
                }
            });
        }

        // Shutdown the ProcessPoolExecutor with GIL
        #[allow(clippy::needless_borrow)]
        Python::attach(|py| {
            Self::shutdown_pool(py, &self.state.executor.bind(py));
        });

        Ok(())
    }
}

impl Drop for PythonRuntime {
    fn drop(&mut self) {
        // Send shutdown to all sync workers
        for _ in 0..self.worker_count {
            let _ = self.sender.send(RuntimeMessage::Shutdown);
        }

        // Send shutdown to async worker
        if let Some(ref async_sender) = self.async_sender {
            let _ = async_sender.send(AsyncRuntimeMessage::Shutdown);
        }

        // Join all sync worker threads
        for handle in self.workers.drain(..) {
            if let Err(e) = handle.join() {
                tracing::error!("Worker thread panicked: {:?}", e);
            }
        }

        // Join async worker thread
        if let Some(handle) = self.async_worker.take() {
            if let Err(e) = handle.join() {
                tracing::error!("Async worker thread panicked: {:?}", e);
            }
        }
    }
}
