> [!WARNING]
> This is a research-level hobby project in alpha. It is not suitable for production use or any serious application.

# Snaxum

A Rust/Axum web server with Flask-style dynamic Python routing via PyO3.

## Quickstart

You add this to your own Axum server to enable Python endpoints:

```
use axum::{routing::any, Router};
use snaxum::prelude::*;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize Python
    pyo3::Python::initialize();

    // Configure the Python runtime
    let config = SnaxumConfig::builder()
        .python_dir("./python")
        .module("endpoints")
        .module("pool_handlers")
        .pool_workers(4)
        .dispatch_workers(4)
        .build()?;

    let runtime = Arc::new(PythonRuntime::with_config(config)?);

    let app = Router::new()
        .route("/python/{*path}", any(handle_python_request))
        .with_state(runtime);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, app).await?;

    Ok(())
}

```

Now you can add python endpoints in files in `./python/`:

```python
# ./python/endpoints.py
import sys
import polars as pl
from snaxum import route, Request

@route('/python/hello', methods=['GET'])
def hello(request: Request) -> dict:
    """Return a greeting with Python version info."""
    return {
        "message": "Hello from Python",
        "version": f"{sys.version_info.major}.{sys.version_info.minor}.{sys.version_info.micro}"
    }

@route('/python/users/<int:user_id>', methods=['GET'])
def get_user(request: Request) -> dict:
    """Get a user by ID - demonstrates path parameter extraction."""
    user_id = request.path_params['user_id']  # Already converted to int
    return {"user_id": user_id, "name": f"User {user_id}"}

def compute_squares(numbers: list[int]) -> list[int]:
    return [n * n for n in numbers]

@route('/python/pool/compute', methods=['POST'], use_process_pool=True)
def pool_compute(request: Request, pool: ProcessPoolExecutor) -> dict[str, int]:
    """Compute squares of numbers using the process pool (POST with body)."""
    numbers = body.get('numbers', [])
    future = pool.submit(compute_squares, numbers)
    return {"squares": future.result()}
```

- See the `example/` directory for a full working example.
- Routes can go in any python file in `./python/`
- All Python endpoints are registered dynamically at runtime via the `@route` decorator.
- Virtual environments are supported for dependency management.

## Goal

The goal is to allow developers to write HTTP endpoints either in Rust or Python,
for the Axum webserver:

- Rust endpoints are compiled, high-performance, and type-safe
- Python endpoints are dynamic, easy to write, and support rich libraries

In practice, Rust-focused developers are likely to use the Rust endpoints, and Python developers the Python endpoints.
Having both allows a pathway for easy and rapid experimentation in Python, with the option to later port performance-critical endpoints to Rust.
A very typical use-case is that researchers and data scientists want to quickly prototype web APIs in Python, using libraries like Pandas, NumPy, or machine learning frameworks.
If an endpoint becomes performance-critical, it can be re-written in Rust later, by Rust experts, while having access to the working reference implementation in Python right there in the same server codebase.

Because the server itself, the entrypoint, is written in Rust, the performance ceiling is much higher than a pure Python server where one might try to improve performance by adding native extensions in Rust, but will discover that the performance ceiling is low due to Python's inherent performance limitations.

This POC is demonstrating an architecture that aims to combine the best of both worlds.

## Project Structure

```
snaxum/
├── src/
│   ├── main.rs              # Server setup, signal handling
│   ├── rust_handlers.rs     # Pure Rust endpoints
│   ├── python_runtime.rs    # Python thread with channel communication
│   └── dispatcher.rs        # Catch-all handler for /python/*
├── python/
│   ├── snaxum.py      # Framework: @route decorator, Request, dispatch
│   ├── endpoints.py         # In-thread Python handlers
│   ├── pool_handlers.py     # Process pool handlers
│   └── pool_workers.py      # Process pool worker functions
├── Cargo.toml
└── pyproject.toml
```

## Developer Setup

### Prerequisites

- Rust (via rustup)
- Python 3.10+ (system install with shared library in standard path)
- uv (Python package manager): `curl -LsSf https://astral.sh/uv/install.sh | sh`
- just (command runner): Available via your package manager

### Quick Start

```bash
# Create venv and install dependencies
just setup

# Start the server
just serve

# Run tests (in another terminal, while server is running)
just test-all
```

### Python Environment Details

This project uses **system Python** for the venv (required for PyO3 runtime linking) and **uv** for package management:

- **Create venv**: `just venv` (uses `/usr/bin/python3 -m venv`)
- **Sync dependencies**: `just sync` (or `uv sync`)
- **Lock dependencies**: `just lock` (or `uv lock`)

> **Why system Python?** PyO3 needs `libpython` in a standard library path at runtime. System Python's libraries are in `/usr/lib64/`, which is already in the linker search path. uv-managed Python installations keep their libraries in non-standard locations.

The `just serve` command automatically configures:
- `PYO3_PYTHON` - points PyO3 to the venv's Python interpreter
- `PYTHONPATH` - adds the venv's site-packages so embedded Python can import installed packages

### Adding Python Dependencies

1. Add the dependency to `pyproject.toml` under `[project.dependencies]`
2. Run `just lock` to update `uv.lock`
3. Run `just sync` to install

### Production Deployment

For production, the deployment system must:

1. Create a Python 3.10+ virtual environment (with system Python for libpython access)
2. Install dependencies from `pyproject.toml` (or use `uv.lock` for reproducible builds)
3. Set `PYO3_PYTHON` to point to the venv's Python interpreter
4. Set `PYTHONPATH` to the venv's site-packages directory

Example Dockerfile pattern:
```dockerfile
# Use system Python for venv (libpython must be in standard path)
RUN python3 -m venv /app/.venv
RUN uv sync --frozen
ENV PYO3_PYTHON=/app/.venv/bin/python
ENV PYTHONPATH=/app/.venv/lib/python3.12/site-packages
```

---


## Runtime Architecture

The route handlers written in Rust work as normal Axum async handlers.
The Python route handlers are all registered dynamically at runtime via decorators in Python code.
The following diagram illustrates the request flow for Python endpoints:

```
HTTP Request → Axum catch-all route (/python/*path)
     → Generic Dispatcher → Channel → Python Runtime Thread
     → snaxum.dispatch(method, path, request_data)
     → Route Registry path matching → User Handler → Response
```

**Key Principle**: Fully dynamic. Rust knows nothing about individual routes - just catches all `/python/*` requests and delegates to Python for path matching and dispatch.

## Python Runtime Thread

A dedicated Rust thread owns the Python GIL and ProcessPoolExecutor:

1. Initializes Python and adds `python/` to `sys.path`
2. Imports `snaxum` framework
3. Imports user modules (registers routes via `@route` decorators)
4. Creates `ProcessPoolExecutor` with worker initializer
5. Logs all registered routes at startup
6. Loops on channel, calling `snaxum.dispatch()` for each request

Communication uses `std::sync::mpsc` for thread-side and `tokio::sync::oneshot` for async responses.

## Two Execution Modes

### Mode 1: In-Thread (Default)

```python
@route('/python/hello', methods=['GET'])
def hello(request: Request) -> dict:
    return {"message": "Hello"}
```

- Handler runs in the Python runtime thread
- GIL held for duration of request
- Use for: quick computations, I/O-bound Python code

### Mode 2: Process Pool

```python
@route('/python/compute', methods=['POST'], use_process_pool=True)
def compute(request: Request, pool: ProcessPoolExecutor) -> dict:
    future = pool.submit(heavy_work, request.body)
    return {"result": future.result()}
```

- Handler receives `ProcessPoolExecutor` as second argument
- True parallelism across cores via separate processes
- Workers ignore SIGINT (main process handles shutdown)
- Use for: CPU-bound work (numpy, pandas, ML inference)

## Flask-Style Routing

### Path Parameters

```python
@route('/python/users/<int:user_id>', methods=['GET'])
def get_user(request: Request) -> dict:
    user_id = request.path_params['user_id']  # Already an int
    return {"user_id": user_id}
```

Supported types: `<int:name>`, `<float:name>`, `<name>` (string)

### Request Object

```python
class Request:
    path_params: Dict[str, Any]    # Extracted from path
    query_params: Dict[str, str]   # ?foo=bar
    headers: Dict[str, str]        # HTTP headers
    body: Optional[Any]            # Parsed JSON body
    method: str                    # GET, POST, etc.
    path: str                      # Full request path
```

## Graceful Shutdown

1. SIGINT/SIGTERM received by Rust signal handler
2. Axum graceful shutdown drains connections
3. `RuntimeMessage::Shutdown` sent to Python thread
4. `ProcessPoolExecutor.shutdown(wait=True, cancel_futures=True)`
5. Python thread exits, joined by main thread
6. Clean exit

## Endpoints

| Path | Handler | Mode |
|------|---------|------|
| `GET /rust/hello` | `rust_handlers::hello` | Rust |
| `POST /rust/echo` | `rust_handlers::echo` | Rust |
| `GET /python/hello` | `endpoints.hello` | In-thread |
| `POST /python/process` | `endpoints.process` | In-thread |
| `GET /python/users/<int:id>` | `endpoints.get_user` | In-thread |
| `GET /python/items/<name>` | `endpoints.get_item` | In-thread |
| `GET /python/echo` | `endpoints.echo` | In-thread |
| `GET /python/polars-demo` | `endpoints.polars_demo` | In-thread |
| `GET /python/pool/squares` | `pool_handlers.pool_squares` | Process pool |
| `POST /python/pool/compute` | `pool_handlers.pool_compute` | Process pool |
| `POST /python/pool/sum` | `pool_handlers.pool_sum` | Process pool |
| `GET /python/pool/factorial/<int:n>` | `pool_handlers.pool_factorial` | Process pool |

## Adding New Endpoints

No rebuild required - just add a decorated function and restart:

```python
# python/endpoints.py
@route('/python/new-feature', methods=['GET', 'POST'])
def new_feature(request: Request) -> dict:
    return {"status": "works"}
```

## Thread Safety and Soundness

### Why Python Cannot Interfere with Rust Endpoints

The architecture guarantees that Python running in its dedicated thread will **never** interfere with Rust endpoint execution:

#### 1. Separate Execution Contexts

| Component | Execution Context |
|-----------|-------------------|
| Rust endpoints | Tokio async task executor (event loop) |
| Python handlers | Dedicated OS thread with GIL |

These are completely independent. Rust async tasks and the Python thread share no execution resources.

#### 2. GIL is Thread-Local

Python's Global Interpreter Lock only affects Python threads. The Rust async code runs on separate OS threads managed by Tokio and is **never** blocked waiting for the GIL:

- `Python::attach()` binds Python context to the dedicated thread only
- Tokio tasks continue executing while Python holds the GIL
- No GIL acquisition happens in Rust endpoint handlers

#### 3. No Shared Mutable State

| Rust Endpoints | Python Endpoints |
|----------------|------------------|
| Access no Python state | Access no Rust handler state |
| Pure async functions | Isolated via channel message passing |
| Independent request/response | Each request gets own oneshot channel |

#### 4. Channel-Based Isolation

Python requests flow through thread-safe channels:

```
HTTP Request → Tokio task → mpsc::Sender → Python thread → oneshot::Sender → Tokio task → HTTP Response
```

- `mpsc::Sender` is `Clone + Send` - safe to share across async tasks
- `oneshot::Receiver` is awaited asynchronously - doesn't block the event loop
- Each request is independent; no shared state in the dispatch path

#### 5. Signal Handling

Signal conflicts are explicitly avoided (`main.rs:17-23`):
- Python's default SIGINT handler is disabled
- Rust handles SIGINT/SIGTERM for graceful shutdown
- ProcessPoolExecutor workers ignore SIGINT

### Verified Guarantees

| Property | Guarantee |
|----------|-----------|
| Rust endpoints blocked by Python | **No** - different execution contexts |
| GIL contention with Rust | **No** - GIL is thread-local |
| Shared state race conditions | **No** - no shared mutable state |
| Signal handler conflicts | **No** - explicitly managed |
| Process pool GIL issues | **No** - separate processes, not threads |

## Key Dependencies

- **axum 0.8**: HTTP routing and server
- **pyo3 0.27**: Rust-Python bindings with `auto-initialize`
- **tokio**: Async runtime with channels for Python communication
