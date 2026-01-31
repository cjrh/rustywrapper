"""
Async runtime for Snaxum - provides a persistent asyncio event loop in a dedicated thread.

This module manages async Python handlers with queue-based communication to Rust.
The async thread runs an asyncio event loop that can handle thousands of concurrent
requests without blocking Rust worker threads.

Communication Pattern:
    Rust -> _request_queue -> Python async thread -> _response_queue -> Rust

GIL Behavior:
    The async thread releases the GIL during I/O operations and while waiting on queues,
    allowing Rust to briefly acquire GIL for queue operations.
"""

import asyncio
import threading
import traceback
from queue import Queue, Empty
from concurrent.futures import ProcessPoolExecutor
from typing import Any

# Queue for incoming requests from Rust
_request_queue: Queue = Queue()

# Queue for completed responses back to Rust
_response_queue: Queue = Queue()

# Async thread handle
_thread: threading.Thread | None = None

# Event loop reference (for external access if needed)
_loop: asyncio.AbstractEventLoop | None = None

# Process pool reference (passed from Rust during init)
_pool: ProcessPoolExecutor | None = None

# Shutdown flag
_shutdown_event: threading.Event = threading.Event()


def start_async_runtime(pool: ProcessPoolExecutor) -> None:
    """
    Start the async worker thread.

    Called once from Rust during initialization.

    Args:
        pool: ProcessPoolExecutor to use for CPU-bound work in async handlers.
    """
    global _thread, _pool
    _pool = pool
    _shutdown_event.clear()
    _thread = threading.Thread(target=_async_thread_main, name="snaxum-async", daemon=True)
    _thread.start()


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
    _request_queue.put((request_id, method, path, data))


def poll_responses() -> list[tuple[int, dict]]:
    """
    Get all completed responses from the async queue.

    Called from Rust periodically to collect completed async responses.
    Non-blocking - returns immediately with whatever is available.

    Returns:
        List of (request_id, response_dict) tuples.
    """
    results = []
    while True:
        try:
            results.append(_response_queue.get_nowait())
        except Empty:
            break
    return results


def shutdown_async_runtime() -> None:
    """
    Signal the async thread to shut down gracefully.

    Called from Rust during runtime shutdown.
    """
    _shutdown_event.set()
    # Send None to wake up the queue.get() if waiting
    _request_queue.put(None)
    if _thread is not None:
        _thread.join(timeout=5.0)


def get_process_pool() -> ProcessPoolExecutor | None:
    """
    Get the ProcessPoolExecutor for use in async handlers.

    Usage in async handlers:
        pool = get_process_pool()
        result = await asyncio.get_event_loop().run_in_executor(pool, cpu_bound_fn, args)

    Returns:
        The ProcessPoolExecutor instance, or None if not initialized.
    """
    return _pool


def _async_thread_main() -> None:
    """
    Main entry point for the async thread.

    Creates a new event loop and runs the main coroutine.
    """
    global _loop
    _loop = asyncio.new_event_loop()
    asyncio.set_event_loop(_loop)

    try:
        _loop.run_until_complete(_main_coroutine())
    finally:
        _loop.close()
        _loop = None


async def _main_coroutine() -> None:
    """
    Main async coroutine that processes requests from the queue.

    Spawns a new task for each incoming request, allowing thousands
    of concurrent handlers.
    """
    loop = asyncio.get_event_loop()

    while not _shutdown_event.is_set():
        try:
            # Wait for request in thread pool to release GIL
            # This allows Rust to acquire GIL for queue operations
            msg = await loop.run_in_executor(None, _blocking_get_request)

            if msg is None:  # Shutdown signal
                break

            request_id, method, path, data = msg

            # Spawn task for this request (non-blocking)
            asyncio.create_task(
                _handle_async_request(request_id, method, path, data)
            )

        except Exception as e:
            # Log but don't crash the loop
            print(f"Error in async main loop: {e}")
            traceback.print_exc()


def _blocking_get_request() -> tuple[int, str, str, dict] | None:
    """
    Blocking get from request queue with timeout.

    Uses a timeout to periodically check the shutdown event.
    """
    while not _shutdown_event.is_set():
        try:
            return _request_queue.get(timeout=0.1)
        except Empty:
            continue
    return None


async def _handle_async_request(
    request_id: int,
    method: str,
    path: str,
    data: dict
) -> None:
    """
    Handle a single async request.

    Dispatches to the appropriate handler and puts the response
    in the response queue.
    """
    try:
        # Import here to avoid circular dependency
        from snaxum import dispatch_async

        response = await dispatch_async(method, path, data, _pool)
        _response_queue.put((request_id, response))

    except Exception as e:
        error_response = {
            "success": False,
            "code": 500,
            "error": str(e),
            "traceback": traceback.format_exc(),
        }
        _response_queue.put((request_id, error_response))
