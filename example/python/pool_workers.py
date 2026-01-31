"""
Process pool worker functions for Snaxum.

These functions run in separate Python processes via ProcessPoolExecutor.
They enable true parallelism for CPU-bound work across multiple cores.

Requirements:
- Functions must be stateless (no shared state between calls)
- Data is serialized across process boundaries (pickle)
- Suitable for heavy data processing (numpy, pandas, scikit-learn)
"""

import signal


def init_worker():
    """Initialize worker process to ignore SIGINT (handled by main process)."""
    _ = signal.signal(signal.SIGINT, signal.SIG_IGN)


def compute_squares(numbers: list[int]) -> list[int]:
    """Compute the square of each number in the input list."""
    return [n * n for n in numbers]


def compute_sum(numbers: list[int]) -> int:
    """Compute the sum of all numbers in the input list."""
    return sum(numbers)


def compute_factorial(n: int) -> int:
    """Compute the factorial of n."""
    if n <= 1:
        return 1
    result = 1
    for i in range(2, n + 1):
        result *= i
    return result


def slow_task(duration: float, task_id: str) -> dict:
    """Simulate a slow task by sleeping for the specified duration.

    Used for testing concurrent request handling.
    """
    import time
    start = time.time()
    time.sleep(duration)
    elapsed = time.time() - start
    return {"task_id": task_id, "duration": duration, "actual_elapsed": round(elapsed, 3)}
