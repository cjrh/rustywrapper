use crate::python_runtime::PythonRuntime;
use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, Method, StatusCode},
    response::IntoResponse,
    Json,
};
use std::collections::HashMap;
use std::sync::Arc;

/// Handle incoming requests to Python routes.
///
/// This is the main Axum handler for the `/python/{*path}` route. It extracts
/// request data and dispatches to the appropriate Python handler via the runtime.
///
/// # Example
///
/// ```ignore
/// use axum::{routing::any, Router};
/// use snaxum::{handle_python_request, PythonRuntime, SnaxumConfig};
/// use std::sync::Arc;
///
/// let config = SnaxumConfig::builder()
///     .python_dir("./python")
///     .module("endpoints")
///     .build()?;
///
/// let runtime = Arc::new(PythonRuntime::with_config(config)?);
///
/// let app = Router::new()
///     .route("/python/{*path}", any(handle_python_request))
///     .with_state(runtime);
/// ```
pub async fn handle_python_request(
    method: Method,
    State(runtime): State<Arc<PythonRuntime>>,
    Path(path): Path<String>,
    Query(query_params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    // Prepend /python to get the full path the handler expects
    let full_path = format!("/python/{}", path);

    // Parse body as JSON if present, otherwise null
    let body_json: Option<serde_json::Value> = if body.is_empty() {
        None
    } else {
        match serde_json::from_slice(&body) {
            Ok(v) => Some(v),
            Err(_) => {
                // Try as string if not valid JSON
                String::from_utf8(body.to_vec())
                    .ok()
                    .map(serde_json::Value::String)
            }
        }
    };

    let request_data = serde_json::json!({
        "query_params": query_params,
        "headers": headers_to_map(&headers),
        "body": body_json,
    });

    match runtime
        .dispatch(method.as_str(), &full_path, request_data)
        .await
    {
        Ok(result) if result.success => (
            StatusCode::OK,
            Json(result.data.unwrap_or(serde_json::Value::Null)),
        )
            .into_response(),
        Ok(result) => {
            let code =
                StatusCode::from_u16(result.code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            (
                code,
                Json(serde_json::json!({
                    "error": result.error.unwrap_or_else(|| "Unknown error".to_string())
                })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

fn headers_to_map(headers: &HeaderMap) -> HashMap<String, String> {
    headers
        .iter()
        .filter_map(|(k, v)| v.to_str().ok().map(|v| (k.to_string(), v.to_string())))
        .collect()
}
