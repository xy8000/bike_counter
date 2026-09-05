use std::str::FromStr;

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Json;
use uuid::Uuid;

use super::{AppState, blocking, map_domain_error};
use crate::adapter::driving::rest::dto::{
    CancelJobRequestDto, ErrorResponseDto, JobDto, JobListDto, JobQueryParams,
};
use crate::core::domain::error::DomainError;
use crate::core::domain::jobs::job::JobStatus;

#[utoipa::path(
    get,
    path = "/api/v1/jobs",
    tag = "Jobs",
    params(JobQueryParams),
    responses(
        (status = 200, description = "List jobs, optionally filtered by job_type and status", body = JobListDto),
        (status = 400, description = "Invalid query parameters", body = ErrorResponseDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn list_jobs(
    State(state): State<AppState>,
    Query(params): Query<JobQueryParams>,
) -> Result<Json<JobListDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let status = match &params.status {
        Some(raw) => Some(JobStatus::from_str(raw).map_err(map_domain_error)?),
        None => None,
    };
    let service = state.job_service.clone();
    let job_type = params.job_type.as_deref().map(str::to_string);
    let jobs = blocking(move || service.list(job_type, status))
        .await
        .map_err(map_domain_error)?;
    Ok(Json(JobListDto::new(jobs)))
}

#[utoipa::path(
    get,
    path = "/api/v1/jobs/{id}",
    tag = "Jobs",
    params(
        ("id" = Uuid, Path, description = "Job UUID")
    ),
    responses(
        (status = 200, description = "Job found", body = JobDto),
        (status = 404, description = "Job not found", body = ErrorResponseDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn get_job_by_id(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<JobDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let service = state.job_service.clone();
    let job = blocking(move || service.find_by_id(id))
        .await
        .map_err(map_domain_error)?;
    Ok(Json(JobDto::from(job)))
}

#[utoipa::path(
    post,
    path = "/api/v1/jobs/{id}/cancel",
    tag = "Jobs",
    params(
        ("id" = Uuid, Path, description = "Job UUID")
    ),
    request_body = CancelJobRequestDto,
    responses(
        (status = 200, description = "Job cancelled (or cancellation requested)", body = JobDto),
        (status = 400, description = "Job is already in a terminal state", body = ErrorResponseDto),
        (status = 404, description = "Job not found", body = ErrorResponseDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn cancel_job(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    body: Bytes,
) -> Result<Json<JobDto>, (StatusCode, Json<ErrorResponseDto>)> {
    // An empty body means `{"force": false}` (cooperative cancel).
    let force = if body.is_empty() {
        false
    } else {
        match serde_json::from_slice::<CancelJobRequestDto>(&body) {
            Ok(request) => request.force,
            Err(_) => {
                return Err(map_domain_error(DomainError::InvalidQuery(
                    "cancel request body must be a JSON object with an optional \"force\" flag"
                        .to_string(),
                )));
            }
        }
    };
    let service = state.job_service.clone();
    let job = blocking(move || service.cancel(id, force))
        .await
        .map_err(map_domain_error)?;
    Ok(Json(JobDto::from(job)))
}
