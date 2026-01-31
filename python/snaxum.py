"""
Snaxum - Flask-style Python routing framework for Rust/Axum integration.

Provides decorator-based route registration with path parameter extraction,
fully dynamic routing at runtime with no build step required.

Supports both sync and async handlers:
    - Sync handlers: Run in Rust dispatch worker threads
    - Async handlers: Run in a dedicated asyncio event loop thread
"""

import inspect
import re
from typing import Dict, Any, List, Optional, Callable, override, TypedDict
from concurrent.futures import ProcessPoolExecutor


class RouteInfo(TypedDict):
    pattern: re.Pattern
    path: str
    method: str
    handler: Callable
    use_process_pool: bool
    is_async: bool
    param_names: list[str]
    param_types: dict[str, type]


_route_registry: list[RouteInfo] = []


class Request:
    """Request object containing all HTTP request data."""

    def __init__(self, data: dict):
        self.path_params: dict[str, Any] = data.get("path_params", {})
        self.query_params: dict[str, str] = data.get("query_params", {})
        self.headers: dict[str, str] = data.get("headers", {})
        self.body: Any | None = data.get("body")
        self.method: str = data.get("method", "GET")
        self.path: str = data.get("path", "/")

    @override
    def __repr__(self) -> str:
        return f"Request(method={self.method!r}, path={self.path!r})"


def route(path: str, methods: list[str] | None = None, use_process_pool: bool = False):
    """
    Decorator to register a handler for a route pattern.

    Args:
        path: Flask-style path pattern (e.g., '/users/<int:id>')
        methods: List of HTTP methods (default: ['GET'])
        use_process_pool: If True, handler receives ProcessPoolExecutor as second arg.
            For async handlers, use get_process_pool() and run_in_executor() instead.

    Examples:
        @route('/hello', methods=['GET'])
        def hello(request: Request) -> dict:
            return {"message": "Hello"}

        @route('/users/<int:user_id>', methods=['GET', 'POST'])
        def user(request: Request) -> dict:
            return {"user_id": request.path_params['user_id']}

        @route('/compute', methods=['POST'], use_process_pool=True)
        def compute(request: Request, pool: ProcessPoolExecutor) -> dict:
            future = pool.submit(heavy_work, request.body)
            return {"result": future.result()}

        @route('/async/io', methods=['GET'])
        async def async_io(request: Request) -> dict:
            await asyncio.sleep(1.0)
            return {"waited": 1.0}

        @route('/async/compute', methods=['POST'])
        async def async_compute(request: Request) -> dict:
            from async_runtime import get_process_pool
            loop = asyncio.get_event_loop()
            pool = get_process_pool()
            result = await loop.run_in_executor(pool, heavy_work, request.body)
            return {"result": result}
    """
    if methods is None:
        methods = ["GET"]

    def decorator(fn: Callable):
        pattern, param_names, param_types = _compile_path_pattern(path)
        is_async = inspect.iscoroutinefunction(fn)
        for method in methods:
            _route_registry.append(
                {
                    "pattern": pattern,
                    "path": path,
                    "method": method.upper(),
                    "handler": fn,
                    "use_process_pool": use_process_pool,
                    "is_async": is_async,
                    "param_names": param_names,
                    "param_types": param_types,
                }
            )
        return fn

    return decorator


def _compile_path_pattern(path: str) -> tuple[re.Pattern, list[str], dict[str, type]]:
    """
    Convert Flask-style path to regex pattern.

    /users/<int:id> → regex pattern, ['id'], {'id': int}
    /items/<name>   → regex pattern, ['name'], {'name': str}
    """
    param_names: list[str] = []
    param_types: dict[str, type] = {}

    def replace_param(match):
        type_hint = match.group(1)
        name = match.group(2)
        param_names.append(name)

        if type_hint == "int":
            param_types[name] = int
            return r"(?P<" + name + r">[0-9]+)"
        elif type_hint == "float":
            param_types[name] = float
            return r"(?P<" + name + r">[0-9.]+)"
        else:  # str or no type hint
            param_types[name] = str
            return r"(?P<" + name + r">[^/]+)"

    # Match <type:name> or <name>
    pattern = re.sub(r"<(?:(\w+):)?(\w+)>", replace_param, path)
    pattern = "^" + pattern + "$"
    return re.compile(pattern), param_names, param_types


def _match_route(method: str, path: str) -> tuple[RouteInfo, dict] | None:
    """Find matching route and extract path params."""
    for route_info in _route_registry:
        if route_info["method"] != method.upper():
            continue
        match = route_info["pattern"].match(path)
        if match:
            # Convert params to declared types
            params = {}
            for name, value in match.groupdict().items():
                converter = route_info["param_types"].get(name, str)
                try:
                    params[name] = converter(value)
                except (ValueError, TypeError):
                    # Type conversion failed, skip this route
                    continue
            return route_info, params
    return None


def dispatch(
    method: str, path: str, request_data: dict, pool: ProcessPoolExecutor = None
) -> dict:
    """
    Main dispatcher for sync handlers - matches path and calls handler.

    Called from Rust for every incoming sync request to /python/*.

    Args:
        method: HTTP method (GET, POST, etc.)
        path: Full request path (e.g., '/python/users/42')
        request_data: Dict with query_params, headers, body
        pool: Optional ProcessPoolExecutor for pool-enabled handlers

    Returns:
        Dict with success/code/data/error fields
    """
    try:
        result = _match_route(method, path)
        if not result:
            return {
                "success": False,
                "code": 404,
                "error": f"No route for {method} {path}",
            }

        route_info, path_params = result

        # Build request object with extracted path params
        request_data["path_params"] = path_params
        request_data["method"] = method
        request_data["path"] = path
        request = Request(request_data)

        handler = route_info["handler"]

        if route_info["use_process_pool"]:
            if pool is None:
                return {
                    "success": False,
                    "code": 500,
                    "error": "ProcessPoolExecutor required but not available",
                }
            response = handler(request, pool)
        else:
            response = handler(request)

        return {"success": True, "data": response}

    except Exception as e:
        import traceback

        return {
            "success": False,
            "code": 500,
            "error": str(e),
            "traceback": traceback.format_exc(),
        }


async def dispatch_async(
    method: str, path: str, request_data: dict, pool: ProcessPoolExecutor = None
) -> dict:
    """
    Async dispatcher - matches path and calls async handler.

    Called from the async runtime for async requests.

    Args:
        method: HTTP method (GET, POST, etc.)
        path: Full request path (e.g., '/python/async/io')
        request_data: Dict with query_params, headers, body
        pool: Optional ProcessPoolExecutor (available via get_process_pool())

    Returns:
        Dict with success/code/data/error fields
    """
    try:
        result = _match_route(method, path)
        if not result:
            return {
                "success": False,
                "code": 404,
                "error": f"No route for {method} {path}",
            }

        route_info, path_params = result

        # Build request object with extracted path params
        request_data["path_params"] = path_params
        request_data["method"] = method
        request_data["path"] = path
        request = Request(request_data)

        handler = route_info["handler"]

        # Async handlers don't receive pool directly - they use get_process_pool()
        response = await handler(request)

        return {"success": True, "data": response}

    except Exception as e:
        import traceback

        return {
            "success": False,
            "code": 500,
            "error": str(e),
            "traceback": traceback.format_exc(),
        }


def get_route_info(method: str, path: str) -> dict | None:
    """
    Get route metadata for a given method and path.

    Called from Rust to determine if a route is async before dispatching.

    Args:
        method: HTTP method (GET, POST, etc.)
        path: Full request path

    Returns:
        Dict with is_async and use_process_pool fields, or None if no route found.
    """
    result = _match_route(method, path)
    if not result:
        return None

    route_info, _ = result
    return {
        "is_async": route_info["is_async"],
        "use_process_pool": route_info["use_process_pool"],
    }


def list_routes() -> list[dict[str, str | bool]]:
    """Return list of registered routes (for debugging/logging)."""
    return [
        {
            "method": r["method"],
            "path": r["path"],
            "handler": r["handler"].__name__,
            "use_process_pool": r["use_process_pool"],
            "is_async": r["is_async"],
        }
        for r in _route_registry
    ]


def has_async_routes() -> bool:
    """Check if any registered routes are async."""
    return any(r["is_async"] for r in _route_registry)


def clear_routes():
    """Clear all registered routes (useful for testing/hot-reload)."""
    global _route_registry
    _route_registry = []
