mod dispatcher;
mod python_runtime;
mod rust_handlers;

use axum::{routing::any, routing::get, routing::post, Router};
use python_runtime::PythonRuntime;
use pyo3::prelude::*;
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

    // Python modules to import (registers their routes via @route decorators)
    let python_modules = &["endpoints", "pool_handlers"];

    // pool_workers: number of ProcessPoolExecutor workers for CPU-bound Python tasks
    // dispatch_workers: number of Rust threads that can dispatch to Python concurrently
    let runtime = Arc::new(
        PythonRuntime::new(4, 4, python_modules).expect("Failed to initialize Python runtime"),
    );
    let runtime_for_shutdown = Arc::clone(&runtime);

    let app = Router::new()
        // Pure Rust routes
        .route("/rust/hello", get(rust_handlers::hello))
        .route("/rust/echo", post(rust_handlers::echo))
        // Catch-all for Python routes - delegates path matching to Python
        .route("/python/{*path}", any(dispatcher::handle_python_request))
        .with_state(runtime);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    tracing::info!("Server listening on http://0.0.0.0:3000");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();

    tracing::info!("Shutting down Python runtime...");
    if let Err(e) = runtime_for_shutdown.shutdown() {
        tracing::error!("Error shutting down runtime: {}", e);
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
