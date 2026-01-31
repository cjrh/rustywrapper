"""Type stubs for the snaxum framework (embedded in Rust binary)."""

from typing import Any, Callable, Coroutine, TypeVar
from concurrent.futures import ProcessPoolExecutor

F = TypeVar("F", bound=Callable[..., Any])

class Request:
    """Request object containing all HTTP request data."""

    path_params: dict[str, Any]
    query_params: dict[str, str]
    headers: dict[str, str]
    body: Any | None
    method: str
    path: str

    def __init__(self, data: dict[str, Any]) -> None: ...
    def __repr__(self) -> str: ...

def route(
    path: str,
    methods: list[str] | None = None,
    use_process_pool: bool = False,
) -> Callable[[F], F]:
    """
    Decorator to register a handler for a route pattern.

    Supports both sync and async handlers. Async handlers are automatically
    detected and routed to the async runtime.

    Args:
        path: Flask-style path pattern (e.g., '/users/<int:id>')
        methods: List of HTTP methods (default: ['GET'])
        use_process_pool: If True, sync handler receives ProcessPoolExecutor as second arg.
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
    ...

def dispatch(
    method: str,
    path: str,
    request_data: dict[str, Any],
    pool: ProcessPoolExecutor | None = None,
) -> dict[str, Any]:
    """
    Main dispatcher - matches path and calls handler.

    Called from Rust for every incoming request to /python/*.

    Args:
        method: HTTP method (GET, POST, etc.)
        path: Full request path (e.g., '/python/users/42')
        request_data: Dict with query_params, headers, body
        pool: Optional ProcessPoolExecutor for pool-enabled handlers

    Returns:
        Dict with success/code/data/error fields
    """
    ...

def list_routes() -> list[dict[str, str | bool]]:
    """Return list of registered routes (for debugging/logging)."""
    ...

def clear_routes() -> None:
    """Clear all registered routes (useful for testing/hot-reload)."""
    ...

async def dispatch_async(
    method: str,
    path: str,
    request_data: dict[str, Any],
    pool: ProcessPoolExecutor | None = None,
) -> dict[str, Any]:
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
    ...

def get_route_info(method: str, path: str) -> dict[str, Any] | None:
    """
    Get route metadata for a given method and path.

    Called from Rust to determine if a route is async before dispatching.

    Args:
        method: HTTP method (GET, POST, etc.)
        path: Full request path

    Returns:
        Dict with is_async and use_process_pool fields, or None if no route found.
    """
    ...

def has_async_routes() -> bool:
    """Check if any registered routes are async."""
    ...
