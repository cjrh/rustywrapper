"""Type stubs for the snaxum framework (embedded in Rust binary)."""

from typing import Any, Callable, TypeVar
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

    Args:
        path: Flask-style path pattern (e.g., '/users/<int:id>')
        methods: List of HTTP methods (default: ['GET'])
        use_process_pool: If True, handler receives ProcessPoolExecutor as second arg

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
