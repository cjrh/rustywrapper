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
