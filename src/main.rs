mod python_handlers;
mod python_pool;
mod rust_handlers;

use axum::{routing::get, routing::post, Router};
use pyo3::prelude::*;
use python_pool::PyProcessPool;
use std::sync::Arc;
use tokio::signal;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    pyo3::Python::initialize();

    // Disable Python's SIGINT handler so Rust handles shutdown
    Python::attach(|py| {
        let signal_mod = py.import("signal").unwrap();
        let sigint = signal_mod.getattr("SIGINT").unwrap();
        let sig_dfl = signal_mod.getattr("SIG_DFL").unwrap();
        signal_mod.call_method1("signal", (sigint, sig_dfl)).unwrap();
    });

    let pool = Arc::new(PyProcessPool::new(4).expect("Failed to create Python process pool"));
    let pool_for_shutdown = Arc::clone(&pool);

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

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();

    tracing::info!("Shutting down Python process pool...");
    if let Err(e) = pool_for_shutdown.shutdown() {
        tracing::error!("Error shutting down pool: {}", e);
    }
    tracing::info!("Shutdown complete");
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("Shutdown signal received");
}
