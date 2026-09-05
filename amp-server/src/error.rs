//! API error type: every failure maps to a JSON body `{ "error": code,
//! "message": human-readable }` so clients (and the web UI) can branch on
//! `error` and show `message` directly to players.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("{0}")]
    BadRequest(String),
    #[error("not authenticated")]
    Unauthorized,
    #[error("{0}")]
    Forbidden(String),
    #[error("{0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("staking is not enabled on this deployment")]
    StakingDisabled,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            ApiError::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            ApiError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            ApiError::Forbidden(_) => (StatusCode::FORBIDDEN, "forbidden"),
            ApiError::NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
            ApiError::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
            ApiError::StakingDisabled => (StatusCode::NOT_IMPLEMENTED, "staking_disabled"),
            ApiError::Database(e) => {
                tracing::error!(error = %e, "database error");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal")
            }
            ApiError::Internal(e) => {
                tracing::error!(error = format!("{e:#}"), "internal error");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal")
            }
        };
        let message = match &self {
            ApiError::Database(_) | ApiError::Internal(_) => "internal server error".to_string(),
            other => other.to_string(),
        };
        (status, Json(json!({ "error": code, "message": message }))).into_response()
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
