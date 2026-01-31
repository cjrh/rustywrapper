# Example Web Server with Rust and Python Endpoints

TODO

## Endpoints

| Path | Handler | Mode |
|------|---------|------|
| `GET /rust/hello` | `rust_handlers::hello` | Rust |
| `POST /rust/echo` | `rust_handlers::echo` | Rust |
| `GET /python/hello` | `endpoints.hello` | In-thread |
| `POST /python/process` | `endpoints.process` | In-thread |
| `GET /python/users/<int:id>` | `endpoints.get_user` | In-thread |
| `GET /python/items/<name>` | `endpoints.get_item` | In-thread |
| `GET /python/echo` | `endpoints.echo` | In-thread |
| `GET /python/polars-demo` | `endpoints.polars_demo` | In-thread |
| `GET /python/pool/squares` | `pool_handlers.pool_squares` | Process pool |
| `POST /python/pool/compute` | `pool_handlers.pool_compute` | Process pool |
| `POST /python/pool/sum` | `pool_handlers.pool_sum` | Process pool |
| `GET /python/pool/factorial/<int:n>` | `pool_handlers.pool_factorial` | Process pool |

