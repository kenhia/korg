//! HTTP error wrapper: wraps any `anyhow::Error` as a JSON body, mapping
//! recognized typed domain errors to 4xx and everything else to 500.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

pub struct ApiError(pub anyhow::Error);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        (status, Json(json!({ "error": self.0.to_string() }))).into_response()
    }
}

impl ApiError {
    /// Map the underlying error to an HTTP status. Typed domain errors
    /// (`PlanningError`, `RepoError`) become 4xx; everything else is 500
    /// (WI #289 — validation/not-found no longer masquerade as server errors).
    fn status(&self) -> StatusCode {
        use korg_core::daily_plan::PlanningError;
        use korg_core::repo::RepoError;

        if let Some(e) = self.0.downcast_ref::<PlanningError>() {
            return match e {
                PlanningError::SourceNotFound(_) | PlanningError::ItemNotFound(_) => {
                    StatusCode::NOT_FOUND
                }
                PlanningError::WrongSource { .. }
                | PlanningError::TargetPast
                | PlanningError::InvalidRange(_) => StatusCode::BAD_REQUEST,
                PlanningError::FrozenPast | PlanningError::InvalidReorder => StatusCode::CONFLICT,
                PlanningError::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
            };
        }
        if let Some(e) = self.0.downcast_ref::<RepoError>() {
            return match e {
                RepoError::InvalidInput(_) => StatusCode::BAD_REQUEST,
                RepoError::NotFound(_) => StatusCode::NOT_FOUND,
            };
        }
        StatusCode::INTERNAL_SERVER_ERROR
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
