//! HTTP error wrapper: wraps any `anyhow::Error` as a JSON body, mapping the
//! core error taxonomy to a status and a stable machine-readable `code`.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

use korg_core::error::{ErrorClass, ErrorCode};

pub struct ApiError(pub anyhow::Error);

impl ApiError {
    /// A caller-facing 400 — use for parse/validation failures that happen at
    /// the transport edge, before korg-core sees them.
    pub fn invalid(msg: impl Into<String>) -> Self {
        Self(korg_core::error::RepoError::invalid(msg).into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let code = self.0.code();
        let body = json!({ "error": self.0.to_string(), "code": code.as_str() });
        (status_for(code), Json(body)).into_response()
    }
}

/// Codes → statuses (D-5). Everything korg-core doesn't classify is a genuine
/// server fault; before WI #524 that bucket also held bad dates, unknown
/// reports and invalid t-shirt sizes.
fn status_for(code: ErrorCode) -> StatusCode {
    match code {
        ErrorCode::InvalidInput => StatusCode::BAD_REQUEST,
        ErrorCode::NotFound => StatusCode::NOT_FOUND,
        ErrorCode::Conflict => StatusCode::CONFLICT,
        ErrorCode::Internal => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

impl<E> From<E> for ApiError
where
    E: Into<anyhow::Error>,
{
    fn from(e: E) -> Self {
        Self(e.into())
    }
}
