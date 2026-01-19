# RustyWrapper Architecture

A Rust/Axum web server where endpoints can be implemented in either Rust or Python via PyO3.

## Project Structure

```
rustywrapper/
├── src/
│   ├── main.rs              # Server setup, signal handling
│   ├── rust_handlers.rs     # Pure Rust endpoints
│   ├── python_handlers.rs   # Python endpoint wrappers
│   └── python_pool.rs       # ProcessPoolExecutor management
├── python/
│   ├── endpoints.py         # In-thread Python functions
│   └── pool_workers.py      # Process pool workers
├── Cargo.toml
└── pyproject.toml
```

## Python Runtime

**Initialization**: `Python::initialize()` called once at startup before creating the tokio runtime.

**Signal Handling**: Python's SIGINT handler is replaced with `SIG_DFL` so Rust owns shutdown behavior. SIGTERM uses default behavior.

## Two Execution Modes

### Mode 1: In-Thread (GIL)

```
Request → tokio::spawn_blocking → Python::attach → python/endpoints.py → Response
```

- Python runs in tokio's blocking thread pool
- GIL acquired per-request via `Python::attach()`
- Sequential Python execution, but Rust async continues
- Use for: quick computations, I/O-bound Python code

### Mode 2: Process Pool

```
Request → spawn_blocking → Python::attach → ProcessPoolExecutor.submit() → Response
```

- `ProcessPoolExecutor` created at startup, stored in app state
- Workers ignore SIGINT (main process handles shutdown)
- True parallelism across cores
- Use for: CPU-bound work (numpy, pandas, ML inference)

## Graceful Shutdown

1. SIGINT/SIGTERM received by Rust signal handler
2. Axum graceful shutdown drains connections
3. `ProcessPoolExecutor.shutdown(wait=True, cancel_futures=True)`
4. Clean exit

## Endpoints

| Path | Handler | Mode |
|------|---------|------|
| `GET /rust/hello` | `rust_handlers::hello` | Rust |
| `POST /rust/echo` | `rust_handlers::echo` | Rust |
| `GET /python/hello` | `python_handlers::hello` | In-thread |
| `POST /python/process` | `python_handlers::process` | In-thread |
| `GET /python/pool/squares` | `python_handlers::pool_squares` | Process pool |
| `POST /python/pool/compute` | `python_handlers::pool_compute` | Process pool |

## Key Dependencies

- **axum 0.8**: HTTP routing and server
- **pyo3 0.27**: Rust-Python bindings
- **tokio**: Async runtime with `spawn_blocking` for Python calls
