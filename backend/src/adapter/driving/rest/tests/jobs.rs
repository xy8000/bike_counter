//! Tests for the `/api/v1/jobs` endpoints.

use axum::http::StatusCode;
use serde_json::json;
use uuid::Uuid;

use crate::adapter::driving::rest::tests::TestApp;
use crate::adapter::driving::rest::tests::assert_not_found;
use crate::adapter::driving::rest::tests::fixtures::{JOB_ID_A, UNKNOWN_ID, job_a, job_b};
use crate::adapter::driving::rest::tests::mocks::MockJobRepository;
use crate::core::domain::jobs::job::{Job, JobStatus};

/// A FAILED job of another type used to exercise the query filters.
fn report_job() -> Job {
    let mut job = Job::new(
        UNKNOWN_ID,
        "Report".to_string(),
        "report_generation".to_string(),
        chrono::Utc::now() + chrono::Duration::hours(1),
    );
    job.status = JobStatus::Failed;
    job.started_at = Some(chrono::Utc::now() - chrono::Duration::minutes(10));
    job.finished_at = Some(chrono::Utc::now());
    job
}

#[tokio::test]
async fn lists_jobs_with_hateoas_links_and_lifetime_fields() {
    let app = TestApp::new();

    let (status, body) = app.get_json("/api/v1/jobs").await;

    assert_eq!(status, StatusCode::OK);
    let items = body["items"].as_array().expect("items should be an array");
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["job_type"], "data_source_update");
    assert_eq!(items[0]["status"], "FINISHED");
    assert_eq!(items[0]["max_lifetime_exceeded"], json!(false));
    assert!(
        items[0]["lifetime_until"].is_string(),
        "lifetime_until should be serialized as a timestamp"
    );
    assert_eq!(
        items[0]["_links"]["self"]["href"],
        format!("/api/v1/jobs/{JOB_ID_A}")
    );
    assert_eq!(body["_links"]["self"]["href"], "/api/v1/jobs");
}

#[tokio::test]
async fn returns_empty_list_when_no_jobs_exist() {
    let app = TestApp::with_jobs(MockJobRepository::new(Vec::new()));

    let (status, body) = app.get_json("/api/v1/jobs").await;

    assert_eq!(status, StatusCode::OK);
    assert!(body["items"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn filters_jobs_by_job_type_and_status() {
    let app = TestApp::with_jobs(MockJobRepository::new(vec![job_a(), job_b(), report_job()]));

    let (_, by_type) = app
        .get_json("/api/v1/jobs?job_type=data_source_update")
        .await;
    assert_eq!(by_type["items"].as_array().unwrap().len(), 2);

    let (_, running) = app.get_json("/api/v1/jobs?status=RUNNING").await;
    assert_eq!(running["items"].as_array().unwrap().len(), 1);
    assert_eq!(running["items"][0]["status"], "RUNNING");

    let (_, both) = app
        .get_json("/api/v1/jobs?job_type=report_generation&status=FAILED")
        .await;
    assert_eq!(both["items"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn rejects_invalid_status_with_400() {
    let app = TestApp::new();

    let (status, body) = app.get_json("/api/v1/jobs?status=BOGUS").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"].as_str().unwrap_or_default().contains("BOGUS"),
        "error should mention the invalid status"
    );
}

#[tokio::test]
async fn gets_job_by_id() {
    let app = TestApp::new();

    let (status, body) = app.get_json(&format!("/api/v1/jobs/{JOB_ID_A}")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], json!(JOB_ID_A.to_string()));
    assert_eq!(body["name"], "Data source update");
    assert_eq!(body["status"], "FINISHED");
    assert!(body["lifetime_until"].is_string());
    assert_eq!(body["max_lifetime_exceeded"], json!(false));
    assert_eq!(
        body["_links"]["self"]["href"],
        format!("/api/v1/jobs/{JOB_ID_A}")
    );
}

#[tokio::test]
async fn returns_404_when_job_does_not_exist() {
    let app = TestApp::new();

    assert_not_found(&app, "/api/v1/jobs", Uuid::new_v4()).await;
}

#[tokio::test]
async fn root_exposes_jobs_link() {
    let app = TestApp::new();

    let (status, body) = app.get_json("/api/v1").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["_links"]["jobs"]["href"], "/api/v1/jobs");
}

#[tokio::test]
async fn openapi_document_contains_jobs_paths_and_schemas() {
    let app = TestApp::new();

    let (status, body) = app.get_json("/api-docs/openapi.json").await;

    assert_eq!(status, StatusCode::OK);
    let paths = body["paths"]
        .as_object()
        .expect("paths should be an object");
    assert!(
        paths.contains_key("/api/v1/jobs"),
        "OpenAPI document should contain /api/v1/jobs"
    );
    assert!(
        paths.contains_key("/api/v1/jobs/{id}"),
        "OpenAPI document should contain /api/v1/jobs/{{id}}"
    );
    let schemas = body["components"]["schemas"]
        .as_object()
        .expect("schemas should be an object");
    assert!(schemas.contains_key("JobDto"));
    assert!(schemas.contains_key("JobStatusDto"));
    assert!(schemas.contains_key("JobListDto"));
}
