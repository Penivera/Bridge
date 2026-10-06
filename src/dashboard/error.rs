use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};

#[derive(Debug, thiserror::Error)]
pub enum DashboardError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    Unavailable(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    Validation(String),
    #[error("{0}")]
    Internal(String),
}

impl DashboardError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::Io(_) | Self::Internal(_) | Self::Json(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::Validation(_) => StatusCode::UNPROCESSABLE_ENTITY,
        }
    }

    pub fn error_code(&self) -> &'static str {
        match self {
            Self::Io(_) | Self::Internal(_) => "INTERNAL_ERROR",
            Self::Json(_) => "JSON_ERROR",
            Self::NotFound(_) => "NOT_FOUND",
            Self::BadRequest(_) => "BAD_REQUEST",
            Self::Unavailable(_) => "UNAVAILABLE",
            Self::Conflict(_) => "CONFLICT",
            Self::Validation(_) => "VALIDATION_ERROR",
        }
    }
}

impl IntoResponse for DashboardError {
    fn into_response(self) -> Response {
        let body = Json(serde_json::json!({
            "error": self.to_string(),
            "code": self.error_code(),
        }));
        (self.status_code(), body).into_response()
    }
}
