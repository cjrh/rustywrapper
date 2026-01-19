"""
Process pool worker functions for RustyWrapper.

These functions run in separate Python processes via ProcessPoolExecutor.
They enable true parallelism for CPU-bound work across multiple cores.

Requirements:
- Functions must be stateless (no shared state between calls)
- Data is serialized across process boundaries (pickle)
- Suitable for heavy data processing (numpy, pandas, scikit-learn)
"""

import signal
from typing import List


def init_worker():
    """Initialize worker process to ignore SIGINT (handled by main process)."""
    signal.signal(signal.SIGINT, signal.SIG_IGN)


def compute_squares(numbers: List[int]) -> List[int]:
    """Compute the square of each number in the input list."""
    return [n * n for n in numbers]


def compute_sum(numbers: List[int]) -> int:
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
