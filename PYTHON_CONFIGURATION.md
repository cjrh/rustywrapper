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
./target/debug/snaxum
cd /tmp && /path/to/snaxum
cargo run
```

With a relative path like `"python"`, the server would only work when run from the project root.

### Import Resolution Order

When Python encounters `from snaxum import route`:

1. `{project_root}/python/` (our inserted path) → finds `snaxum.py` ✓
2. Standard library paths (`/usr/lib/python3.x/`, etc.)
3. Site-packages (pip-installed packages)

## Module Structure

```
python/
├── snaxum.py      # Framework: @route decorator, Request class, dispatch()
├── endpoints.py         # In-thread request handlers
├── pool_handlers.py     # ProcessPoolExecutor-based handlers
└── pool_workers.py      # Functions that run in worker processes
```

### Import Patterns

**Within the python/ directory**, modules import each other using simple absolute imports:

```python
# pool_handlers.py
from snaxum import route, Request
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

## Type Hints and IDE Support

Since `snaxum.py` is embedded into the Rust binary at compile time, it doesn't exist as a resolvable Python module for type checkers. To enable IDE autocompletion and type checking, we use stub files.

### Setup

The `example/` directory includes:

- `typings/snaxum.pyi` - Type stubs for the snaxum module
- `pyrightconfig.json` - Pyright configuration pointing to the stubs

### What's Provided

The stub file exports type information for:

- `Request` class with typed attributes (`path_params`, `query_params`, `headers`, `body`, `method`, `path`)
- `@route` decorator with full signature and docstring
- `dispatch()`, `list_routes()`, and `clear_routes()` functions

### Verification

To verify type hints are working:

1. Open `example/python/endpoints.py` in VS Code
2. Hover over `Request` - should show class with typed attributes
3. Hover over `@route` - should show decorator signature with docstring
4. Type `request.` and verify autocomplete shows all attributes
5. Run `pyright example/python/` to verify no type errors

### For New Projects

Copy the `typings/` directory and `pyrightconfig.json` to your project:

```bash
cp -r example/typings your-project/
cp example/pyrightconfig.json your-project/
```

Adjust paths in `pyrightconfig.json` if your Python code is in a different directory.

## Troubleshooting

### "ModuleNotFoundError: No module named 'snaxum'"

The server was likely started from the wrong directory, or the `python/` directory doesn't exist.

**Fix**: Run from the project root, or check that `python/snaxum.py` exists.

### "ModuleNotFoundError: No module named 'endpoints'"

A module listed in `python_modules` doesn't exist in the `python/` directory.

**Fix**: Check the module name in `main.rs` matches the actual filename (without `.py`):

```rust
let python_modules = &["endpoints", "pool_handlers"];  // → python/endpoints.py, python/pool_handlers.py
```

### Import works in Python REPL but not in server

The REPL might have a different working directory or `sys.path`. The server logs its Python module path at startup:

```
INFO snaxum::python_runtime: Python module path: /absolute/path/to/project/python
```

Verify this path is correct.
