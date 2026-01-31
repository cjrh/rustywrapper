"""Type stubs for the async_runtime module (embedded in Rust binary)."""

from concurrent.futures import ProcessPoolExecutor

def start_async_runtime(pool: ProcessPoolExecutor) -> None:
    """
    Start the async worker thread.

    Called once from Rust during initialization.

    Args:
        pool: ProcessPoolExecutor to use for CPU-bound work in async handlers.
    """
    ...

def submit_async(request_id: int, method: str, path: str, data: dict) -> None:
    """
    Submit a request to the async queue.

    Called from Rust for each incoming async request.

    Args:
        request_id: Unique identifier for correlating response.
        method: HTTP method (GET, POST, etc.).
        path: Full request path.
        data: Request data dict (query_params, headers, body).
    """
    ...

def poll_responses() -> list[tuple[int, dict]]:
    """
    Get all completed responses from the async queue.

    Called from Rust periodically to collect completed async responses.
    Non-blocking - returns immediately with whatever is available.

    Returns:
        List of (request_id, response_dict) tuples.
    """
    ...

def shutdown_async_runtime() -> None:
    """
    Signal the async thread to shut down gracefully.

    Called from Rust during runtime shutdown.
    """
    ...

def get_process_pool() -> ProcessPoolExecutor | None:
    """
    Get the ProcessPoolExecutor for use in async handlers.

    Usage in async handlers:
        pool = get_process_pool()
        result = await asyncio.get_event_loop().run_in_executor(pool, cpu_bound_fn, args)

    Returns:
        The ProcessPoolExecutor instance, or None if not initialized.
    """
    ...
