"""
Async Python endpoints for Snaxum demonstrating async handler patterns.

These handlers run in a dedicated asyncio event loop thread, allowing
thousands of concurrent requests without blocking Rust worker threads.
"""

import asyncio
import sys
from concurrent.futures import ProcessPoolExecutor
from snaxum import route, Request


@route('/python/async/hello', methods=['GET'])
async def async_hello(request: Request) -> dict:
    """Basic async handler - demonstrates async function detection."""
    return {
        "message": "Hello from async Python!",
        "version": f"{sys.version_info.major}.{sys.version_info.minor}.{sys.version_info.micro}"
    }


@route('/python/async/sleep', methods=['GET'])
async def async_sleep(request: Request) -> dict:
    """
    Async sleep endpoint - demonstrates non-blocking I/O wait.

    Query params:
        duration: Sleep duration in seconds (default: 1.0)

    Example:
        curl "http://localhost:3000/python/async/sleep?duration=2.5"
    """
    duration = float(request.query_params.get('duration', '1.0'))
    await asyncio.sleep(duration)
    return {
        "message": f"Slept for {duration} seconds",
        "duration": duration
    }


@route('/python/async/concurrent', methods=['GET'])
async def async_concurrent(request: Request) -> dict:
    """
    Demonstrates concurrent async operations using asyncio.gather().

    Makes multiple "requests" concurrently that would take 3 seconds
    sequentially but only ~1 second concurrently.

    Example:
        curl http://localhost:3000/python/async/concurrent
    """
    async def simulated_io(name: str, delay: float) -> dict:
        await asyncio.sleep(delay)
        return {"name": name, "delay": delay}

    # These run concurrently, so total time is max(delays), not sum(delays)
    results = await asyncio.gather(
        simulated_io("request_1", 1.0),
        simulated_io("request_2", 1.0),
        simulated_io("request_3", 1.0),
    )

    return {
        "message": "Completed 3 operations concurrently",
        "results": list(results)
    }


@route('/python/async/compute', methods=['POST'], use_process_pool=True)
async def async_compute(request: Request, pool: ProcessPoolExecutor) -> dict:
    """
    Demonstrates CPU-bound work in async handler using ProcessPoolExecutor.

    For CPU-bound work in async handlers, use run_in_executor() to offload
    to the process pool without blocking the event loop.

    Body:
        JSON with 'count' field for number of iterations

    Example:
        curl -X POST http://localhost:3000/python/async/compute \\
             -H "Content-Type: application/json" \\
             -d '{"count": 1000000}'
    """
    count = request.body.get('count', 100000) if request.body else 100000

    # Run CPU-bound work in the process pool
    loop = asyncio.get_event_loop()
    result = await loop.run_in_executor(pool, _cpu_bound_work, count)

    return {
        "message": "CPU-bound work completed",
        "iterations": count,
        "result": result
    }


def _cpu_bound_work(count: int) -> int:
    """CPU-bound work that runs in process pool."""
    total = 0
    for i in range(count):
        total += i * i
    return total


@route('/python/async/echo', methods=['GET', 'POST'])
async def async_echo(request: Request) -> dict:
    """Echo back request details - async version."""
    return {
        "async": True,
        "method": request.method,
        "path": request.path,
        "query_params": request.query_params,
        "body": request.body,
    }


@route('/python/async/timeout', methods=['GET'])
async def async_timeout(request: Request) -> dict:
    """
    Demonstrates asyncio.wait_for() timeout pattern.

    Query params:
        timeout: Timeout in seconds (default: 1.0)
        delay: Simulated operation delay (default: 2.0)

    Example (will timeout):
        curl "http://localhost:3000/python/async/timeout?timeout=1&delay=2"

    Example (will succeed):
        curl "http://localhost:3000/python/async/timeout?timeout=2&delay=1"
    """
    timeout = float(request.query_params.get('timeout', '1.0'))
    delay = float(request.query_params.get('delay', '2.0'))

    async def slow_operation():
        await asyncio.sleep(delay)
        return {"completed": True}

    try:
        result = await asyncio.wait_for(slow_operation(), timeout=timeout)
        return {
            "success": True,
            "result": result,
            "timeout": timeout,
            "delay": delay
        }
    except asyncio.TimeoutError:
        return {
            "success": False,
            "error": f"Operation timed out after {timeout}s (delay was {delay}s)",
            "timeout": timeout,
            "delay": delay
        }
