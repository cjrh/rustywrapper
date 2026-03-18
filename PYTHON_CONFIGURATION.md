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
./target/debug/chimera
cd /tmp && /path/to/chimera
cargo run
```

With a relative path like `"python"`, the server would only work when run from the project root.

### Import Resolution Order

When Python encounters `from chimera import route`:

1. `{project_root}/python/` (our inserted path) → finds `chimera.py` ✓
2. Standard library paths (`/usr/lib/python3.x/`, etc.)
3. Site-packages (pip-installed packages)

## Module Structure

```
python/
├── chimera.py      # Framework: @route decorator, Request class, dispatch()
├── endpoints.py         # In-thread request handlers
├── pool_handlers.py     # ProcessPoolExecutor-based handlers
└── pool_workers.py      # Functions that run in worker processes
```

### Import Patterns

**Within the python/ directory**, modules import each other using simple absolute imports:

```python
# pool_handlers.py
from chimera import route, Request
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

PyO3's `auto-initialize` embeds the Python interpreter directly in the Rust process. Unlike running `python` from a shell, this bypasses normal venv activation: no `pyvenv.cfg` is read, `sys.prefix` isn't redirected, and venv `site-packages` aren't on `sys.path`. The result is that only stdlib imports work unless the runtime explicitly adds the venv's site-packages to `sys.path`.

Chimera handles this automatically when it knows where your venv is.

### Configuration

Set the `VIRTUAL_ENV` environment variable to your venv root. Chimera reads it at startup, resolves `lib/python*/site-packages`, and inserts it into `sys.path` before importing any user modules.

Note: `VIRTUAL_ENV` alone does nothing for embedded interpreters — PyO3 ignores it entirely. Chimera reads the variable and performs the `sys.path` insertion itself.

### Minimal Justfile Example

```just
venv_dir := justfile_directory() / ".venv"
venv_python := venv_dir / "bin/python"

serve: venv sync
    PYO3_PYTHON={{venv_python}} VIRTUAL_ENV={{venv_dir}} cargo run
```

Both variables are derived from the same `venv_dir` — single source of truth. `PYO3_PYTHON` tells PyO3 which Python to link against at build time. `VIRTUAL_ENV` tells Chimera where to find packages at runtime.

### Programmatic Override: `.venv()`

For cases where environment variables aren't suitable (multi-venv setups, testing), use the `.venv()` builder method to override `VIRTUAL_ENV` with an explicit path:

```rust
let config = ChimeraConfig::builder()
    .python_dir("python")
    .venv(".venv")  // overrides VIRTUAL_ENV
    .module("endpoints")
    .build()?;
```

## Python Distribution Requirements

The venv must be created with a Python interpreter that has a **shared libpython** (`libpython3.x.so`) accessible to the dynamic linker.

### Recommended: System Python

System/distro Python (`/usr/bin/python3`) is the simplest option. On most Linux distributions, it ships with `--enable-shared` and places `libpython3.x.so` in a standard linker path. Create the venv with system Python, then use `uv` for fast package management:

```bash
/usr/bin/python3 -m venv --without-pip .venv
uv sync  # or: uv pip install polars numpy etc.
```

### Incompatible: `uv`-provided interpreters

Python interpreters installed by `uv python install` use [python-build-standalone](https://github.com/indygreg/python-build-standalone) distributions, which are **not compatible** with PyO3 embedding:

- They statically link libpython and all extensions into the interpreter binary
- PyO3 needs a shared `libpython3.x.so` to link against at build time and load at runtime
- PyO3's `auto-initialize` feature is explicitly disabled when static linking is detected
- Extension modules (numpy, polars, etc.) expect a loadable shared libpython — static linking breaks this
- Workarounds like `LD_LIBRARY_PATH` are fragile and don't address the fundamental static-vs-shared mismatch

**Relevant issues**: [uv#11006](https://github.com/astral-sh/uv/issues/11006), [python-build-standalone#619](https://github.com/indygreg/python-build-standalone/issues/619), [PyO3#1568](https://github.com/PyO3/pyo3/issues/1568)

## Type Hints and IDE Support

Since `chimera.py` is embedded into the Rust binary at compile time, it doesn't exist as a resolvable Python module for type checkers. To enable IDE autocompletion and type checking, we use stub files.

### Setup

The `example/` directory includes:

- `typings/chimera.pyi` - Type stubs for the chimera module
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

### "ModuleNotFoundError: No module named 'chimera'"

The server was likely started from the wrong directory, or the `python/` directory doesn't exist.

**Fix**: Run from the project root, or check that `python/chimera.py` exists.

### "ModuleNotFoundError: No module named 'endpoints'"

A module listed in `python_modules` doesn't exist in the `python/` directory.

**Fix**: Check the module name in `main.rs` matches the actual filename (without `.py`):

```rust
let python_modules = &["endpoints", "pool_handlers"];  // → python/endpoints.py, python/pool_handlers.py
```

### "ModuleNotFoundError" for pip-installed packages (polars, numpy, etc.)

The embedded Python interpreter can't find packages installed in a virtual environment.

**Fix**: Set `VIRTUAL_ENV` to your venv root when running the server (e.g., `VIRTUAL_ENV=.venv cargo run`). Chimera reads this and adds the venv's `site-packages` to `sys.path` at startup. See [Virtual Environment Support](#virtual-environment-support).

### "libpython not found" or linking errors at runtime

The venv was likely created with a Python that doesn't have a shared `libpython3.x.so`.

**Fix**: Recreate the venv using system Python (`/usr/bin/python3 -m venv`), not a `uv`-installed interpreter. See [Python Distribution Requirements](#python-distribution-requirements).

### Import works in Python REPL but not in server

The REPL might have a different working directory or `sys.path`. The server logs its Python module path at startup:

```
INFO chimera::python_runtime: Python module path: /absolute/path/to/project/python
INFO chimera::python_runtime: Venv site-packages: /absolute/path/to/.venv/lib/python3.x/site-packages
```

Verify these paths are correct.
