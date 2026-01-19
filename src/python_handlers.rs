use crate::python_pool::PyProcessPool;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use pyo3::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Serialize)]
pub struct PythonHelloResponse {
    message: String,
    version: String,
}

pub async fn hello() -> impl IntoResponse {
    let result = tokio::task::spawn_blocking(|| {
        Python::attach(|py| {
            let sys = py.import("sys")?;
            sys.getattr("path")?.call_method1("insert", (0, "python"))?;

            let endpoints = py.import("endpoints")?;
            let result = endpoints.call_method0("hello")?;

            let message: String = result.get_item("message")?.extract()?;
            let version: String = result.get_item("version")?.extract()?;

            Ok::<_, PyErr>(PythonHelloResponse { message, version })
        })
    })
    .await
    .unwrap();

    match result {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
pub struct ProcessRequest {
    data: serde_json::Value,
}

#[derive(Serialize)]
pub struct ProcessResponse {
    result: serde_json::Value,
    source: String,
}

pub async fn process(Json(payload): Json<ProcessRequest>) -> impl IntoResponse {
    let data_str = payload.data.to_string();

    let result = tokio::task::spawn_blocking(move || {
        Python::attach(|py| {
            let sys = py.import("sys")?;
            sys.getattr("path")?.call_method1("insert", (0, "python"))?;

            let json_module = py.import("json")?;
            let py_data = json_module.call_method1("loads", (&data_str,))?;

            let endpoints = py.import("endpoints")?;
            let result = endpoints.call_method1("process", (py_data,))?;

            let result_json = json_module.call_method1("dumps", (result,))?;
            let result_str: String = result_json.extract()?;

            let parsed: serde_json::Value = serde_json::from_str(&result_str)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(e.to_string()))?;

            Ok::<_, PyErr>(ProcessResponse {
                result: parsed,
                source: "python".to_string(),
            })
        })
    })
    .await
    .unwrap();

    match result {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
pub struct SquaresQuery {
    numbers: String,
}

#[derive(Serialize)]
pub struct SquaresResponse {
    squares: Vec<i64>,
}

pub async fn pool_squares(
    State(pool): State<Arc<PyProcessPool>>,
    Query(query): Query<SquaresQuery>,
) -> impl IntoResponse {
    let numbers: Result<Vec<i64>, _> = query
        .numbers
        .split(',')
        .map(|s| s.trim().parse::<i64>())
        .collect();

    let numbers = match numbers {
        Ok(nums) => nums,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid numbers format"})),
            )
                .into_response()
        }
    };

    let pool_clone = Arc::clone(&pool);
    let result = tokio::task::spawn_blocking(move || pool_clone.compute_squares(numbers))
        .await
        .unwrap();

    match result {
        Ok(squares) => (StatusCode::OK, Json(SquaresResponse { squares })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
pub struct ComputeRequest {
    numbers: Vec<i64>,
}

pub async fn pool_compute(
    State(pool): State<Arc<PyProcessPool>>,
    Json(payload): Json<ComputeRequest>,
) -> impl IntoResponse {
    let pool_clone = Arc::clone(&pool);
    let result = tokio::task::spawn_blocking(move || pool_clone.compute_squares(payload.numbers))
        .await
        .unwrap();

    match result {
        Ok(squares) => (StatusCode::OK, Json(SquaresResponse { squares })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}
