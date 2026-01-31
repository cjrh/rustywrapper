> [!WARNING]
> This is a research-level hobby project in alpha. It is not suitable for production use or any serious application.

# RustyWrapper Architecture

A Rust/Axum web server with Flask-style dynamic Python routing via PyO3.

## Goal

The goal is to allow developers to write endpoints either in Rust or Python.
- Rust endpoints are compiled, high-performance, and type-safe
- Python endpoints are dynamic, easy to write, and support rich libraries

In practice, Rust developers are likely to use the Rust endpoints, and Python developers the Python endpoints.
Having both allows a pathway for easy and rapid experimentation in Python, with the option to later port performance-critical endpoints to Rust.
Because the server itself, the entrypoint, is written in Rust, the performance ceiling is much higher than a pure Python server where one might try to improve performance by adding native extensions in Rust, but will discover that the performance ceiling is low due to Python's inherent performance limitations.

This POC is demonstrating an architecture that aims to combine the best of both worlds.

## Project Structure

```
rustywrapper/
├── src/
│   ├── main.rs              # Server setup, signal handling
│   ├── rust_handlers.rs     # Pure Rust endpoints
│   ├── python_runtime.rs    # Python thread with channel communication
│   └── dispatcher.rs        # Catch-all handler for /python/*
├── python/
│   ├── rustywrapper.py      # Framework: @route decorator, Request, dispatch
│   ├── endpoints.py         # In-thread Python handlers
│   ├── pool_handlers.py     # Process pool handlers
│   └── pool_workers.py      # Process pool worker functions
├── Cargo.toml
└── pyproject.toml
```

## Runtime Architecture

The route handlers written in Rust work as normal Axum async handlers.
The Python route handlers are all registered dynamically at runtime via decorators in Python code.
The following diagram illustrates the request flow for Python endpoints:

```
HTTP Request → Axum catch-all route (/python/*path)
     → Generic Dispatcher → Channel → Python Runtime Thread
     → rustywrapper.dispatch(method, path, request_data)
     → Route Registry path matching → User Handler → Response
```

**Key Principle**: Fully dynamic. Rust knows nothing about individual routes - just catches all `/python/*` requests and delegates to Python for path matching and dispatch.

## Python Runtime Thread

A dedicated Rust thread owns the Python GIL and ProcessPoolExecutor:

1. Initializes Python and adds `python/` to `sys.path`
2. Imports `rustywrapper` framework
3. Imports user modules (registers routes via `@route` decorators)
4. Creates `ProcessPoolExecutor` with worker initializer
5. Logs all registered routes at startup
6. Loops on channel, calling `rustywrapper.dispatch()` for each request

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
