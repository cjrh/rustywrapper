"""
Process pool endpoints for RustyWrapper using Flask-style decorators.

These handlers use ProcessPoolExecutor for true CPU parallelism.
The pool is passed as the second argument when use_process_pool=True.
"""

from concurrent.futures import ProcessPoolExecutor
from rustywrapper import route, Request
from pool_workers import compute_squares, compute_sum, compute_factorial


@route('/python/pool/squares', methods=['GET'], use_process_pool=True)
def pool_squares(request: Request, pool: ProcessPoolExecutor) -> dict:
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


@route('/python/pool/compute', methods=['POST'], use_process_pool=True)
def pool_compute(request: Request, pool: ProcessPoolExecutor) -> dict:
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


@route('/python/pool/sum', methods=['POST'], use_process_pool=True)
def pool_sum(request: Request, pool: ProcessPoolExecutor) -> dict:
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


@route('/python/pool/factorial/<int:n>', methods=['GET'], use_process_pool=True)
def pool_factorial(request: Request, pool: ProcessPoolExecutor) -> dict:
    """Compute the factorial of n using the process pool."""
    n = request.path_params['n']

    if n < 0:
        return {"error": "Factorial not defined for negative numbers", "result": None}
    if n > 1000:
        return {"error": "Number too large (max 1000)", "result": None}

    future = pool.submit(compute_factorial, n)
    result = future.result()

    # Convert to string for large numbers to avoid JSON issues
    return {"n": n, "result": str(result) if n > 20 else result}
