use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

#[derive(Debug)]
pub struct Failure {
    pub status: StatusCode,
    pub code: &'static str,
}

impl Failure {
    pub const fn new(status: StatusCode, code: &'static str) -> Self {
        Self { status, code }
    }
}

pub type ApiResult<T> = Result<T, Failure>;

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        (self.status, Json(serde_json::json!({ "error": self.code }))).into_response()
    }
}

impl From<sqlx::Error> for Failure {
    fn from(error: sqlx::Error) -> Self {
        tracing::error!(%error, "database error");
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal")
    }
}
