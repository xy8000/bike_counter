//! Tests for the `/api/v1/jobs` endpoints.

use axum::http::{Method, StatusCode};
use serde_json::json;
use uuid::Uuid;

use crate::adapter::driving::rest::tests::TestApp;
use crate::adapter::driving::rest::tests::assert_not_found;
use crate::adapter::driving::rest::tests::fixtures::{
    JOB_ID_A, JOB_ID_B, JOB_INSTANCE, UNKNOWN_ID, job_a, job_b,
};
use crate::adapter::driving::rest::tests::mocks::MockJobRepository;
use crate::core::domain::jobs::job::{Job, JobStatus};

/// A FAILED job of another type used to exercise the query filters.
fn report_job() -> Job {
    let mut job = Job::running(
        UNKNOWN_ID,
        "Report".to_string(),
        "report_generation".to_string(),
        JOB_INSTANCE,
        chrono::Utc::now() - chrono::Duration::minutes(10),
    );
    job.status = JobStatus::Failed;
    job.finished_at = Some(chrono::Utc::now());
    job
}

#[tokio::test]
async fn lists_jobs_with_hateoas_links_and_ownership_fields() {
    let app = TestApp::new();

    let (status, body) = app.get_json("/api/v1/jobs").await;

    assert_eq!(status, StatusCode::OK);
    let items = body["items"].as_array().expect("items should be an array");
    assert_eq!(items.len(), 2);
    // job_a() is FINISHED and no longer cancellable.
    assert_eq!(items[0]["job_type"], "data_source_update");
    assert_eq!(items[0]["status"], "FINISHED");
    assert_eq!(items[0]["instance_id"], json!(JOB_INSTANCE.to_string()));
    assert!(
        items[0]["heartbeat_at"].is_string(),
        "heartbeat_at should be serialized as a timestamp"
    );
    assert!(
        items[0]["_links"].get("cancel").is_none(),
        "a FINISHED job must not expose a cancel link"
    );
    assert_eq!(
        items[0]["_links"]["self"]["href"],
        format!("/api/v1/jobs/{JOB_ID_A}")
    );
    // The RUNNING job exposes the cancel action.
    assert_eq!(items[1]["status"], "RUNNING");
    assert_eq!(
        items[1]["_links"]["cancel"]["href"],
        format!("/api/v1/jobs/{JOB_ID_B}/cancel")
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

    let (_, cancelled) = app.get_json("/api/v1/jobs?status=CANCELLED").await;
    assert_eq!(cancelled["items"].as_array().unwrap().len(), 0);
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
    assert_eq!(body["instance_id"], json!(JOB_INSTANCE.to_string()));
    assert!(body["heartbeat_at"].is_string());
    assert!(
        body["_links"].get("cancel").is_none(),
        "a FINISHED job must not expose a cancel link"
    );
    assert_eq!(
        body["_links"]["self"]["href"],
        format!("/api/v1/jobs/{JOB_ID_A}")
    );
}

#[tokio::test]
async fn cancel_running_job_requests_cooperative_cancellation() {
    let app = TestApp::new();

    let uri = format!("/api/v1/jobs/{JOB_ID_B}/cancel");
    let (status, body) = app.request_json(Method::POST, &uri, None).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "CANCELLATION_REQUESTED");
    // A requested cancellation can still be force-cancelled, so the cancel link
    // remains exposed.
    assert_eq!(
        body["_links"]["cancel"]["href"],
        format!("/api/v1/jobs/{JOB_ID_B}/cancel")
    );

    // A subsequent GET reflects the persisted transition.
    let (_, fetched) = app.get_json(&format!("/api/v1/jobs/{JOB_ID_B}")).await;
    assert_eq!(fetched["status"], "CANCELLATION_REQUESTED");
}

#[tokio::test]
async fn force_cancel_marks_a_running_job_cancelled() {
    let app = TestApp::new();

    let uri = format!("/api/v1/jobs/{JOB_ID_B}/cancel");
    let (status, body) = app
        .request_json(Method::POST, &uri, Some(json!({ "force": true })))
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "CANCELLED");
}

#[tokio::test]
async fn cancelling_a_terminal_job_is_a_400() {
    let app = TestApp::new();

    let (status, body) = app
        .request_json(
            Method::POST,
            &format!("/api/v1/jobs/{JOB_ID_A}/cancel"),
            None,
        )
        .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("terminal"),
        "error should explain that the job is already terminal"
    );
}

#[tokio::test]
async fn cancelling_an_unknown_job_is_a_404() {
    let app = TestApp::new();

    let uri = format!("/api/v1/jobs/{UNKNOWN_ID}/cancel");
    let (status, body) = app.request_json(Method::POST, &uri, None).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains(&UNKNOWN_ID.to_string()),
        "error should mention the unknown id"
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
    assert!(
        paths.contains_key("/api/v1/jobs/{id}/cancel"),
        "OpenAPI document should contain /api/v1/jobs/{{id}}/cancel"
    );
    let schemas = body["components"]["schemas"]
        .as_object()
        .expect("schemas should be an object");
    assert!(schemas.contains_key("JobDto"));
    assert!(schemas.contains_key("JobStatusDto"));
    assert!(schemas.contains_key("JobListDto"));
    assert!(schemas.contains_key("CancelJobRequestDto"));
}
