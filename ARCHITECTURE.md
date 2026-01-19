# RustyWrapper Architecture

A Rust/Axum web server with Flask-style dynamic Python routing via PyO3.

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

```
HTTP Request → Axum catch-all route (/python/*path)
     → Generic Dispatcher → Channel → Python Runtime Thread
     → rustywrapper.dispatch(method, path, request_data)
     → Route Registry path matching → User Handler → Response
```

**Key Principle**: Fully dynamic. Rust knows nothing about individual routes - just catches all `/python/*` requests and delegates to Python for path matching and dispatch.

## Python Runtime Thread

A dedicated thread owns the Python GIL and ProcessPoolExecutor:

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

## Key Dependencies

- **axum 0.8**: HTTP routing and server
- **pyo3 0.27**: Rust-Python bindings with `auto-initialize`
- **tokio**: Async runtime with channels for Python communication
