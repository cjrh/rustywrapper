# RustyWrapper development commands

# Start the server
serve:
    cargo run

# Build the project
build:
    cargo build

# Test all endpoints (server must be running)
test-all: test-rust test-python test-pool

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
