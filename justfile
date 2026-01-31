# RustyWrapper development commands

# Absolute path to venv for PyO3
venv_dir := justfile_directory() / ".venv"
venv_python := venv_dir / "bin/python"

# Export environment variables for PyO3 and Python paths
export PYO3_PYTHON := venv_python
export PYTHONPATH := shell(venv_python + ' -c "import site; print(site.getsitepackages()[0])"')

@default:
    just --list

# Python venv management
#
# This is a pickle. PyO3 works great with venvs, but only if the libpython
# shared library is in a standard location. If you use a system
# Python to create the venv, libpython nevertheless remains in a standard
# location and at runtime the linker has no problem finding it.
# The problem arises when you use a different pythonn distribution
# that does not have libpython accessible in a standard location.
# We're using uv, which has this problem. If we used uv to create
# the venv, then at runtime PyO3 would not be able to find libpython
# We could resolve the issue by setting LD_LIBRARY_PATH, but that
# is messy and error-prone. So instead we create the venv
# using the system Python, which works fine. All the other commands
# nevertheless use uv.
venv:
    /usr/bin/python3 -m venv --without-pip .venv

sync:
    uv sync

lock:
    uv lock

# Full dev setup from scratch
setup: venv sync
    @echo "Development environment ready!"
    @echo "Run 'just serve' to start the server"

# Start the server (uses venv Python for PyO3)
serve:
    cargo run

# Build the project (uses venv Python for PyO3)
build:
    cargo build

# Test all endpoints (server must be running)
test-all: test-rust test-python test-pool test-concurrency \
    test-python-polars

# Test Rust endpoints
test-rust:
    @echo "Testing Rust endpoints..."
    curl -s http://localhost:3000/rust/hello | jq
    curl -s -X POST http://localhost:3000/rust/echo \
        -H "Content-Type: application/json" \
        -d '{"name":"test"}' | jq

# Test Python in-thread endpoints
test-python:
    @echo "Testing Python in-thread endpoints..."
    curl -s http://localhost:3000/python/hello | jq
    curl -s -X POST http://localhost:3000/python/process \
        -H "Content-Type: application/json" \
        -d '{"data":"hello"}' | jq

# Test python endpoints with a venv
test-python-polars:
    curl -s http://localhost:3000/python/polars-demo | jq

# Test Python process pool endpoints
test-pool:
    @echo "Testing Python process pool endpoints..."
    curl -s "http://localhost:3000/python/pool/squares?numbers=1,2,3,4,5" | jq
    curl -s -X POST http://localhost:3000/python/pool/compute \
        -H "Content-Type: application/json" \
        -d '{"numbers":[1,2,3,4,5]}' | jq

# Test concurrent request handling
# Sends 4 requests that each sleep 1s. With concurrency, completes in ~1-2s (not ~4s)
test-concurrency:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Testing concurrent request handling..."
    echo "Sending 4 requests that each sleep 1 second..."
    echo ""

    start_time=$(date +%s.%N)

    # Send 4 concurrent requests
    curl -s "http://localhost:3000/python/pool/slow?duration=1&task_id=1" > /tmp/task1.json &
    pid1=$!
    curl -s "http://localhost:3000/python/pool/slow?duration=1&task_id=2" > /tmp/task2.json &
    pid2=$!
    curl -s "http://localhost:3000/python/pool/slow?duration=1&task_id=3" > /tmp/task3.json &
    pid3=$!
    curl -s "http://localhost:3000/python/pool/slow?duration=1&task_id=4" > /tmp/task4.json &
    pid4=$!

    # Wait for all to complete
    wait $pid1 $pid2 $pid3 $pid4

    end_time=$(date +%s.%N)
    elapsed=$(echo "$end_time - $start_time" | bc)

    echo "Results:"
    for i in 1 2 3 4; do
        echo "  Task $i: $(jq -c . /tmp/task$i.json)"
    done
    echo ""
    echo "Total wall-clock time: ${elapsed}s"

    # Check if concurrent (should be < 3s for 4x1s tasks)
    if (( $(echo "$elapsed < 3.0" | bc -l) )); then
        echo "✓ PASS: Requests processed concurrently (< 3s)"
    else
        echo "✗ FAIL: Requests processed sequentially (expected < 3s, got ${elapsed}s)"
        exit 1
    fi
