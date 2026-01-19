# Python Configuration

This document describes how Python is configured and integrated with the Rust runtime.

## Module Search Path

Python modules in the `python/` directory are made importable by adding the directory to `sys.path` at runtime.

### How It Works

In `src/python_runtime.rs`, when the Python runtime thread starts:

```rust
// Compute absolute path to python/ directory before spawning thread
let python_dir = std::env::current_dir()
    .map_err(|e| RuntimeError::Python(format!("Failed to get current directory: {}", e)))?
    .join("python");

// Inside the Python thread:
let sys = py.import("sys")?;
let path = sys.getattr("path")?;
path.call_method1("insert", (0, &python_dir))?;
```

This inserts the absolute path to `python/` at position 0 of `sys.path`, making it the first location Python searches when resolving imports.

### Why Absolute Paths?

Using an absolute path (computed from `std::env::current_dir()`) ensures imports work regardless of where the server binary is invoked from:

```bash
# All of these work with absolute paths:
./target/debug/rustywrapper
cd /tmp && /path/to/rustywrapper
cargo run
```

With a relative path like `"python"`, the server would only work when run from the project root.

### Import Resolution Order

When Python encounters `from rustywrapper import route`:

1. `{project_root}/python/` (our inserted path) → finds `rustywrapper.py` ✓
2. Standard library paths (`/usr/lib/python3.x/`, etc.)
3. Site-packages (pip-installed packages)

## Module Structure

```
python/
├── rustywrapper.py      # Framework: @route decorator, Request class, dispatch()
├── endpoints.py         # In-thread request handlers
├── pool_handlers.py     # ProcessPoolExecutor-based handlers
└── pool_workers.py      # Functions that run in worker processes
```

### Import Patterns

**Within the python/ directory**, modules import each other using simple absolute imports:

```python
# pool_handlers.py
from rustywrapper import route, Request
from pool_workers import compute_squares, compute_sum
```

These work because `python/` is in `sys.path`.

## ProcessPoolExecutor and Imports

Worker processes spawned by `ProcessPoolExecutor` inherit the parent's `sys.path`, so imports work correctly in worker functions:

```python
# pool_workers.py - runs in separate processes
def compute_squares(numbers: list[int]) -> list[int]:
    # Can import from python/ directory if needed
    return [n * n for n in numbers]
```

**Important**: Worker functions must be defined in a separate module (`pool_workers.py`) from the handlers that submit to the pool. This is a Python multiprocessing requirement - the function must be importable by the worker process.

## Virtual Environment Support

*Coming soon*

Future versions will support:
- Automatic virtualenv activation
- Configuration via `pyproject.toml` or environment variables
- Example integrations with numpy, scipy, and polars

## Troubleshooting

### "ModuleNotFoundError: No module named 'rustywrapper'"

The server was likely started from the wrong directory, or the `python/` directory doesn't exist.

**Fix**: Run from the project root, or check that `python/rustywrapper.py` exists.

### "ModuleNotFoundError: No module named 'endpoints'"

A module listed in `python_modules` doesn't exist in the `python/` directory.

**Fix**: Check the module name in `main.rs` matches the actual filename (without `.py`):

```rust
let python_modules = &["endpoints", "pool_handlers"];  // → python/endpoints.py, python/pool_handlers.py
```

### Import works in Python REPL but not in server

The REPL might have a different working directory or `sys.path`. The server logs its Python module path at startup:

```
INFO rustywrapper::python_runtime: Python module path: /absolute/path/to/project/python
```

Verify this path is correct.
