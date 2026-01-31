//! Example server demonstrating Snaxum usage.
//!
//! Run with: cargo run (from example/ directory)
//!
//! Then test with:
//! - curl http://localhost:3000/rust/hello
//! - curl http://localhost:3000/python/hello
//! - curl -X POST http://localhost:3000/python/process -d '{"data":"test"}'

use axum::{
    http::StatusCode,
    response::IntoResponse,
    routing::{any, get, post},
    Json, Router,
};
use snaxum::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::signal;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    pyo3::Python::initialize();

    // Configure the Python runtime
    let config = SnaxumConfig::builder()
        .python_dir("python")
        .module("endpoints")
        .module("pool_handlers")
        .pool_workers(4)
        .dispatch_workers(4)
        .build()
        .expect("Failed to build config");

    let runtime = Arc::new(
        PythonRuntime::with_config(config).expect("Failed to initialize Python runtime"),
    );
    let runtime_for_shutdown = Arc::clone(&runtime);

    let app = Router::new()
        // Pure Rust routes
        .route("/rust/hello", get(hello))
        .route("/rust/echo", post(echo))
        // Catch-all for Python routes - delegates path matching to Python
        .route("/python/{*path}", any(handle_python_request))
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

// ============================================================================
// Rust Handlers
// ============================================================================

#[derive(Serialize)]
struct HelloResponse {
    message: String,
    timestamp: u64,
}

async fn hello() -> impl IntoResponse {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    Json(HelloResponse {
        message: "Hello from Rust".to_string(),
        timestamp,
    })
}

#[derive(Deserialize)]
struct EchoRequest {
    #[serde(flatten)]
    data: serde_json::Value,
}

#[derive(Serialize)]
struct EchoResponse {
    echo: serde_json::Value,
    source: String,
}

async fn echo(Json(payload): Json<EchoRequest>) -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(EchoResponse {
            echo: payload.data,
            source: "rust".to_string(),
        }),
    )
}
