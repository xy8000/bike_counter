//! Integration tests for the OpenData tree under `/api/v1/opendata`.
//!
//! The files are produced by the **real** generator adapter and published into
//! an in-memory registry + keyed object storage, then fetched through the router
//! (byte-exact body, `Content-Type`, `Content-Length`, `ETag`, `If-None-Match →
//! 304`, `404` for missing files) and the JSON indices/metadata are asserted.

use axum::http::header::ETAG;
use axum::http::{Method, StatusCode};
use chrono::NaiveDate;
use http_body_util::BodyExt;
use serde_json::Value;
use sha2::{Digest, Sha256 as Sha2Digest};
use std::sync::Arc;
use uuid::Uuid;

use super::TestApp;
use super::fixtures::STATION_ID_A;
use super::mocks::{MockObjectStorage, MockOpenDataFileRepository, sample_opendata_storage};
use crate::adapter::driven::opendata_file_generator::OpendataFileGenerator;
use crate::core::application::opendata_service::OpenDataService;
use crate::core::domain::assets::asset_storage_port::AssetStorage;
use crate::core::domain::opendata::file::{Format, Granularity, OpenDataFile, object_key};
use crate::core::domain::opendata::file_generator_port::OpenDataFileGenerator;
use crate::core::domain::opendata::file_repository_port::OpenDataFileRepository;
use crate::core::domain::opendata::measurement::OpenDataMeasurement;
use crate::core::domain::opendata::service_port::OpenDataServicePort;

// A station that exists in the sample counting-station service fixtures.
const STATION: Uuid = STATION_ID_A;

fn sha256_hex(bytes: &[u8]) -> String {
    Sha2Digest::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn rows() -> Vec<OpenDataMeasurement> {
    vec![
        OpenDataMeasurement {
            station_id: STATION,
            channel_id: Uuid::from_u128(2),
            channel_name: "Channel A".to_string(),
            timestamp: NaiveDate::from_ymd_opt(2026, 9, 5)
                .unwrap()
                .and_hms_opt(14, 0, 0)
                .unwrap(),
            value: 42,
            resolution_seconds: 3600,
        },
        OpenDataMeasurement {
            station_id: STATION,
            channel_id: Uuid::from_u128(3),
            channel_name: "Channel B".to_string(),
            timestamp: NaiveDate::from_ymd_opt(2026, 9, 5)
                .unwrap()
                .and_hms_opt(15, 0, 0)
                .unwrap(),
            value: 7,
            resolution_seconds: 3600,
        },
    ]
}

/// Generates real bytes for one format via the generator adapter.
fn generate_bytes(format: Format) -> Vec<u8> {
    OpendataFileGenerator
        .generate(&rows(), format)
        .expect("generation succeeds")
}

/// Publishes a file (real bytes + registry row) for the given scope/period.
fn publish(
    repo: &MockOpenDataFileRepository,
    storage: &MockObjectStorage,
    station_id: Option<Uuid>,
    granularity: Granularity,
    period: &str,
    format: Format,
) -> OpenDataFile {
    let bytes = generate_bytes(format);
    let key = object_key(station_id, granularity, period, format);
    storage.put_bytes(&key, bytes.clone());
    let file = OpenDataFile {
        id: Uuid::new_v4(),
        object_key: key.clone(),
        station_id,
        granularity,
        period: period.to_string(),
        format,
        byte_size: bytes.len() as i64,
        sha256: sha256_hex(&bytes),
        created_at: chrono::Utc::now(),
    };
    repo.insert(&file).unwrap();
    file
}

/// A router with the given registry/storage seeded.
fn app(repo: Arc<MockOpenDataFileRepository>, storage: Arc<MockObjectStorage>) -> TestApp {
    let service: Arc<dyn OpenDataServicePort> = Arc::new(OpenDataService::new(repo));
    let storage: Arc<dyn AssetStorage> = storage;
    TestApp::with_opendata(service, storage)
}

/// Reads a raw response body into bytes.
async fn body_bytes(response: axum::response::Response) -> bytes::Bytes {
    response
        .into_body()
        .collect()
        .await
        .expect("body collectable")
        .to_bytes()
}

#[tokio::test]
async fn root_metadata_and_stations_are_served() {
    let app = app(
        Arc::new(MockOpenDataFileRepository::default()),
        Arc::new(MockObjectStorage::default()),
    );

    let (status, body) = app.get_json("/api/v1/opendata").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["_links"]["metadata"]["href"].as_str().is_some());
    assert_eq!(
        body["_links"]["stations"]["href"],
        "/api/v1/opendata/stations"
    );

    let (status, metadata) = app.get_json("/api/v1/opendata/metadata").await;
    assert_eq!(status, StatusCode::OK);
    assert!(metadata["schemas"]["measurement"]["schema"]["properties"]["station_id"].is_object());
    assert!(metadata["schemas"]["station"]["schema"].is_object());
    assert!(metadata["schemas"]["distribution"]["schema"].is_object());

    let (status, stations) = app.get_json("/api/v1/opendata/stations").await;
    assert_eq!(status, StatusCode::OK);
    let items = stations["stations"].as_array().unwrap();
    assert!(!items.is_empty());
    assert!(items[0]["station_id"].as_str().is_some());
    assert!(
        items[0]["external_datasource_id"].is_null()
            || items[0]["external_datasource_id"].is_string()
    );

    let (status, geojson) = app.get_json("/api/v1/opendata/stations.geojson").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(geojson["type"], "FeatureCollection");
    for feature in geojson["features"].as_array().unwrap() {
        assert_eq!(feature["geometry"]["type"], "Point");
        assert!(feature["properties"]["station_id"].as_str().is_some());
    }
}

#[tokio::test]
async fn daily_year_index_lists_the_registered_distributions() {
    let repo = Arc::new(MockOpenDataFileRepository::default());
    let storage = Arc::new(MockObjectStorage::default());
    let day = "2026-09-05";
    let published = vec![
        publish(
            &repo,
            &storage,
            None,
            Granularity::Daily,
            day,
            Format::Parquet,
        ),
        publish(
            &repo,
            &storage,
            None,
            Granularity::Daily,
            day,
            Format::CsvGz,
        ),
        publish(&repo, &storage, None, Granularity::Daily, day, Format::Json),
    ];
    let app = app(repo, storage);

    let (status, body) = app
        .get_json("/api/v1/opendata/measurements/daily/2026")
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["year"], "2026");
    let files = body["files"].as_array().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0]["date"], day);
    let distributions = files[0]["distributions"].as_array().unwrap();
    assert_eq!(distributions.len(), 3);

    for file in &published {
        let json = distributions
            .iter()
            .find(|d| d["format"] == file.format.as_str())
            .expect("distribution present");
        assert_eq!(json["url"], format!("/api/v1/{}", file.object_key));
        assert_eq!(json["size_bytes"], file.byte_size);
        assert_eq!(json["sha256"], file.sha256);
    }
}

#[tokio::test]
async fn fetches_the_json_file_bytes_with_etag_and_304() {
    let repo = Arc::new(MockOpenDataFileRepository::default());
    let storage = Arc::new(MockObjectStorage::default());
    let published = publish(
        &repo,
        &storage,
        None,
        Granularity::Daily,
        "2026-09-05",
        Format::Json,
    );
    let app = app(repo, storage);

    let file_name = published.object_key.rsplit('/').next().unwrap().to_string();
    let url = format!("/api/v1/opendata/measurements/daily/2026/{file_name}");

    let response = app.send(Method::GET, &url).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "application/json");
    assert_eq!(
        response.headers()["content-length"],
        published.byte_size.to_string()
    );
    let etag = response.headers()[ETAG].to_str().unwrap().to_string();
    assert_eq!(etag, format!("\"{}\"", published.sha256));
    let body = body_bytes(response).await;
    assert_eq!(body.as_ref(), generate_bytes(Format::Json).as_slice());

    // Conditional request: If-None-Match with the same ETag -> 304.
    let response = app
        .send_with_header(Method::GET, &url, "if-none-match", &etag)
        .await;
    assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
}

#[tokio::test]
async fn fetches_csv_gz_and_parquet_files() {
    let repo = Arc::new(MockOpenDataFileRepository::default());
    let storage = Arc::new(MockObjectStorage::default());
    let csv = publish(
        &repo,
        &storage,
        None,
        Granularity::Daily,
        "2026-09-05",
        Format::CsvGz,
    );
    let parquet = publish(
        &repo,
        &storage,
        None,
        Granularity::Daily,
        "2026-09-05",
        Format::Parquet,
    );
    let app = app(repo, storage);

    for (file, expected_type, expected_bytes) in [
        (&csv, "application/gzip", generate_bytes(Format::CsvGz)),
        (
            &parquet,
            "application/vnd.apache.parquet",
            generate_bytes(Format::Parquet),
        ),
    ] {
        let file_name = file.object_key.rsplit('/').next().unwrap().to_string();
        let url = format!("/api/v1/opendata/measurements/daily/2026/{file_name}");
        let response = app.send(Method::GET, &url).await;
        assert_eq!(response.status(), StatusCode::OK, "GET {url}");
        assert_eq!(response.headers()["content-type"], expected_type);
        let body = body_bytes(response).await;
        assert_eq!(body.as_ref(), expected_bytes.as_slice());
    }
}

#[tokio::test]
async fn station_scoped_file_is_fetched_and_indices_list_it() {
    let repo = Arc::new(MockOpenDataFileRepository::default());
    let storage = Arc::new(MockObjectStorage::default());
    let published = publish(
        &repo,
        &storage,
        Some(STATION),
        Granularity::Monthly,
        "2026-09",
        Format::Json,
    );
    let app = app(repo, storage);

    // The station's month index lists the distribution.
    let (status, body) = app
        .get_json(&format!(
            "/api/v1/opendata/stations/{STATION}/measurements/monthly/2026-09"
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["distributions"][0]["url"],
        format!("/api/v1/{}", published.object_key)
    );

    // Fetching the file returns its exact bytes.
    let file_name = published.object_key.rsplit('/').next().unwrap().to_string();
    let url =
        format!("/api/v1/opendata/stations/{STATION}/measurements/monthly/2026-09/{file_name}");
    let response = app.send(Method::GET, &url).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_bytes(response).await;
    assert_eq!(body.as_ref(), generate_bytes(Format::Json).as_slice());

    // Unknown station -> 404.
    let (status, _) = app
        .get_json("/api/v1/opendata/stations/00000000-0000-0000-0000-000000000000/measurements/daily/2026")
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn missing_file_is_404() {
    let app = app(
        Arc::new(MockOpenDataFileRepository::default()),
        Arc::new(MockObjectStorage::default()),
    );
    let (status, _) = app
        .get_json("/api/v1/opendata/measurements/daily/2026/2026-09-05.json")
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = app.get_json("/api/v1/opendata/measurements").await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = app
        .get_json("/api/v1/opendata/measurements/daily/2026")
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["files"], Value::Array(vec![]));
}

#[tokio::test]
async fn etag_headers_on_indices_allow_304() {
    let repo = Arc::new(MockOpenDataFileRepository::default());
    let storage = Arc::new(MockObjectStorage::default());
    publish(
        &repo,
        &storage,
        None,
        Granularity::Daily,
        "2026-09-05",
        Format::Json,
    );
    let app = app(repo, storage);

    let response = app
        .send(Method::GET, "/api/v1/opendata/measurements/daily/2026")
        .await;
    let etag = response.headers()[ETAG].to_str().unwrap().to_string();
    let second = app
        .send_with_header(
            Method::GET,
            "/api/v1/opendata/measurements/daily/2026",
            "if-none-match",
            &etag,
        )
        .await;
    assert_eq!(second.status(), StatusCode::NOT_MODIFIED);
}

#[tokio::test]
async fn sample_opendata_storage_is_usable_as_a_storage() {
    // Ensures the shared storage double is a valid AssetStorage (sanity import).
    let storage = sample_opendata_storage();
    assert!(storage.list_object_keys().is_ok());
}
