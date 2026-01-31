"""
In-thread Python endpoints for RustyWrapper using Flask-style decorators.

These handlers run in the Python runtime thread via channel communication.
The GIL is held for the duration of each request.
"""

import sys
import polars as pl
from rustywrapper import route, Request


@route('/python/hello', methods=['GET'])
def hello(request: Request) -> dict:
    """Return a greeting with Python version info."""
    return {
        "message": "Hello from Python",
        "version": f"{sys.version_info.major}.{sys.version_info.minor}.{sys.version_info.micro}"
    }


@route('/python/process', methods=['POST'])
def process(request: Request) -> dict:
    """Process input data and return a transformed result."""
    data = request.body
    if isinstance(data, dict):
        processed = {k: v.upper() if isinstance(v, str) else v for k, v in data.items()}
    elif isinstance(data, str):
        processed = data.upper()
    elif isinstance(data, list):
        processed = [item.upper() if isinstance(item, str) else item for item in data]
    else:
        processed = data

    return {"processed": processed, "input_type": type(data).__name__}


@route('/python/users/<int:user_id>', methods=['GET'])
def get_user(request: Request) -> dict:
    """Get a user by ID - demonstrates path parameter extraction."""
    user_id = request.path_params['user_id']  # Already converted to int
    return {"user_id": user_id, "name": f"User {user_id}"}


@route('/python/items/<name>', methods=['GET'])
def get_item(request: Request) -> dict:
    """Get an item by name - demonstrates string path parameter."""
    name = request.path_params['name']
    return {"item": name, "found": True}


@route('/python/echo', methods=['GET', 'POST'])
def echo(request: Request) -> dict:
    """Echo back request details - useful for debugging."""
    return {
        "method": request.method,
        "path": request.path,
        "query_params": request.query_params,
        "body": request.body,
    }


@route('/python/polars-demo', methods=['GET'])
def polars_demo(request: Request) -> dict:
    """Demonstrate polars DataFrame creation and serialization."""
    df = pl.DataFrame({
        "name": ["Alice", "Bob", "Charlie"],
        "age": [25, 30, 35],
        "city": ["New York", "London", "Paris"]
    })
    return df.to_dict(as_series=False)
