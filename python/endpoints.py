"""
In-thread Python endpoints for RustyWrapper.

These functions run in Rust threads via tokio's blocking thread pool.
The GIL is acquired when executing, so Python execution is sequential,
but Rust async operations continue unblocked.
"""

import sys


def hello() -> dict:
    """Return a greeting with Python version info."""
    return {
        "message": "Hello from Python",
        "version": f"{sys.version_info.major}.{sys.version_info.minor}.{sys.version_info.micro}",
    }


def process(data) -> dict:
    """Process input data and return a transformed result."""
    if isinstance(data, dict):
        processed = {k: v.upper() if isinstance(v, str) else v for k, v in data.items()}
    elif isinstance(data, str):
        processed = data.upper()
    elif isinstance(data, list):
        processed = [item.upper() if isinstance(item, str) else item for item in data]
    else:
        processed = data

    return {"processed": processed, "input_type": type(data).__name__}
