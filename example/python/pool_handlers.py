"""
Process pool endpoints for Snaxum using Flask-style decorators.

These handlers use ProcessPoolExecutor for true CPU parallelism.
The pool is passed as the second argument when use_process_pool=True.
"""

from concurrent.futures import ProcessPoolExecutor
from snaxum import route, Request
from pool_workers import compute_squares, compute_sum, compute_factorial, slow_task
from typing import Any, TypedDict, NotRequired


class PoolSquaresResponse(TypedDict):
    squares: list[int] | list[str]
    error: NotRequired[str]


@route('/python/pool/squares', methods=['GET'], use_process_pool=True)
def pool_squares(request: Request, pool: ProcessPoolExecutor) -> PoolSquaresResponse:
    """Compute squares of numbers using the process pool (GET with query param)."""
    numbers_str = request.query_params.get('numbers', '')
    if not numbers_str:
        return {"error": "Missing 'numbers' query parameter", "squares": []}

    try:
        numbers = [int(n.strip()) for n in numbers_str.split(',') if n.strip()]
    except ValueError as e:
        return {"error": f"Invalid number format: {e}", "squares": []}

    future = pool.submit(compute_squares, numbers)
    return {"squares": future.result()}


class PoolComputeResponse(TypedDict):
    squares: list[int] | list[str]
    error: NotRequired[str]


@route('/python/pool/compute', methods=['POST'], use_process_pool=True)
def pool_compute(request: Request, pool: ProcessPoolExecutor) -> PoolComputeResponse:
    """Compute squares of numbers using the process pool (POST with body)."""
    body = request.body or {}
    numbers = body.get('numbers', [])

    if not isinstance(numbers, list):
        return {"error": "Expected 'numbers' to be a list", "squares": []}

    try:
        numbers = [int(n) for n in numbers]
    except (ValueError, TypeError) as e:
        return {"error": f"Invalid number in list: {e}", "squares": []}

    future = pool.submit(compute_squares, numbers)
    return {"squares": future.result()}


class PoolSumResponse(TypedDict):
    result: int | None
    error: NotRequired[str]


@route('/python/pool/sum', methods=['POST'], use_process_pool=True)
def pool_sum(request: Request, pool: ProcessPoolExecutor) -> PoolSumResponse:
    """Compute the sum of numbers using the process pool."""
    body = request.body or {}
    numbers = body.get('numbers', [])

    if not isinstance(numbers, list):
        return {"error": "Expected 'numbers' to be a list", "result": None}

    try:
        numbers = [int(n) for n in numbers]
    except (ValueError, TypeError) as e:
        return {"error": f"Invalid number in list: {e}", "result": None}

    future = pool.submit(compute_sum, numbers)
    return {"result": future.result()}


class PoolFactorialResponse(TypedDict):
    n: NotRequired[int]
    result: int | str | None
    error: NotRequired[str]


@route('/python/pool/factorial/<int:n>', methods=['GET'], use_process_pool=True)
def pool_factorial(request: Request, pool: ProcessPoolExecutor) -> PoolFactorialResponse:
    """Compute the factorial of n using the process pool."""
    n: int = request.path_params['n']

    if n < 0:
        return {"error": "Factorial not defined for negative numbers", "result": None}
    if n > 1000:
        return {"error": "Number too large (max 1000)", "result": None}

    future = pool.submit(compute_factorial, n)
    result = future.result()

    # Convert to string for large numbers to avoid JSON issues
    return {"n": n, "result": str(result) if n > 20 else result}


class SlowTaskResponse(TypedDict):
    task_id: str
    duration: float
    actual_elapsed: float
    error: NotRequired[str]


@route('/python/pool/slow', methods=['GET'], use_process_pool=True)
def pool_slow(request: Request, pool: ProcessPoolExecutor) -> SlowTaskResponse:
    """Execute a slow task to test concurrent request handling.

    Query params:
        duration: sleep duration in seconds (default: 1.0, max: 10.0)
        task_id: identifier for tracking (default: "unknown")
    """
    try:
        duration = float(request.query_params.get('duration', '1.0'))
    except ValueError:
        return {"error": "Invalid duration", "task_id": "", "duration": 0, "actual_elapsed": 0}

    duration = min(max(duration, 0.1), 10.0)  # Clamp between 0.1 and 10 seconds
    task_id = request.query_params.get('task_id', 'unknown')

    future = pool.submit(slow_task, duration, task_id)
    return future.result()
