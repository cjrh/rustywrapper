use axum::{http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize)]
pub struct HelloResponse {
    message: String,
    timestamp: u64,
}

pub async fn hello() -> impl IntoResponse {
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
pub struct EchoRequest {
    #[serde(flatten)]
    data: serde_json::Value,
}

#[derive(Serialize)]
pub struct EchoResponse {
    echo: serde_json::Value,
    source: String,
}

pub async fn echo(Json(payload): Json<EchoRequest>) -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(EchoResponse {
            echo: payload.data,
            source: "rust".to_string(),
        }),
    )
}
