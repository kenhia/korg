//! HTTP error wrapper: any `anyhow::Error` becomes a 500 JSON body.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

pub struct ApiError(pub anyhow::Error);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self
            .0
            .downcast_ref::<korg_core::daily_plan::PlanningError>()
            .map(|error| match error {
                korg_core::daily_plan::PlanningError::SourceNotFound(_)
                | korg_core::daily_plan::PlanningError::ItemNotFound(_) => StatusCode::NOT_FOUND,
                korg_core::daily_plan::PlanningError::WrongSource { .. }
                | korg_core::daily_plan::PlanningError::TargetPast
                | korg_core::daily_plan::PlanningError::InvalidRange(_) => StatusCode::BAD_REQUEST,
                korg_core::daily_plan::PlanningError::FrozenPast
                | korg_core::daily_plan::PlanningError::InvalidReorder => StatusCode::CONFLICT,
                korg_core::daily_plan::PlanningError::Database(_) => {
                    StatusCode::INTERNAL_SERVER_ERROR
                }
            })
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (status, Json(json!({ "error": self.0.to_string() }))).into_response()
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
