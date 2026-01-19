mod python_handlers;
mod python_pool;
mod rust_handlers;

use axum::{routing::get, routing::post, Router};
use python_pool::PyProcessPool;
use std::sync::Arc;
use tracing_subscriber;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    pyo3::Python::initialize();

    let pool = Arc::new(PyProcessPool::new(4).expect("Failed to create Python process pool"));

    let app = Router::new()
        .route("/rust/hello", get(rust_handlers::hello))
        .route("/rust/echo", post(rust_handlers::echo))
        .route("/python/hello", get(python_handlers::hello))
        .route("/python/process", post(python_handlers::process))
        .route(
            "/python/pool/squares",
            get(python_handlers::pool_squares),
        )
        .route(
            "/python/pool/compute",
            post(python_handlers::pool_compute),
        )
        .with_state(pool);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    tracing::info!("Server listening on http://0.0.0.0:3000");

    axum::serve(listener, app).await.unwrap();
}
