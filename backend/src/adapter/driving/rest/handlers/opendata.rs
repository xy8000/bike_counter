//! HTTP handlers for the public OpenData tree under `/api/v1/opendata`.
//!
//! The endpoints are read-only: JSON index/metadata payloads and the immutable
//! distribution files (parquet / csv.gz / json), streamed from the dedicated
//! opendata object-storage bucket. S3 is never exposed. JSON responses carry a
//! strong ETag (SHA-256 of the body) and honor `If-None-Match` → `304`; file
//! responses use the file's `sha256` as the ETag and an immutable
//! `Cache-Control`.

use std::collections::HashMap;

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE, ETAG, IF_NONE_MATCH};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{Json, Response};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::adapter::driving::bff::cache::{CachePolicy, cached_json};
use crate::adapter::driving::rest::dto::ErrorResponseDto;
use crate::adapter::driving::rest::handlers::{AppState, blocking, map_domain_error};
use crate::core::domain::assets::asset::value_objects::ObjectKey;
use crate::core::domain::counting_stations::counting_station::CountingStation;
use crate::core::domain::counting_stations::counting_station::value_objects::Id;
use crate::core::domain::error::DomainError;
use crate::core::domain::opendata::file::{Granularity, OpenDataFile};

/// The timezone all opendata files are bucketed and timestamped in.
const TIMEZONE: &str = "Europe/Berlin";
/// Index payloads change only when the export job appends files (once a day).
const INDEX_CACHE: CachePolicy = CachePolicy::Windowed;

fn not_found(message: String) -> (StatusCode, Json<ErrorResponseDto>) {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorResponseDto { error: message }),
    )
}

/// A distribution (one format of one file) with its immutable content metadata.
fn distribution(file: &OpenDataFile) -> Value {
    json!({
        "format": file.format.as_str(),
        "url": format!("/api/v1/{}", file.object_key),
        "size_bytes": file.byte_size,
        "sha256": file.sha256,
    })
}

/// A [`CountingStation`] as the opendata station object (uuid primary id; the
/// provider-native `external_datasource_id` is exposed as an additional note
/// field).
fn station_value(station: &CountingStation) -> Value {
    let coordinates = station
        .coordinates
        .map(|c| json!({ "latitude": c.latitude, "longitude": c.longitude }));
    json!({
        "station_id": station.id.0,
        "name": station.name.0,
        "description": station.description.0,
        "external_datasource_id": station.external_datasource_id.as_ref().map(|id| &id.0),
        "timezone": station.timezone.0,
        "coordinates": coordinates,
        "status": station.status.as_str(),
    })
}

/// The HATEOAS sub-links of the measurements tree for a scope.
fn measurements_links(scope: &str) -> Value {
    json!({
        "self": { "href": format!("/api/v1/opendata/{scope}") },
        "metadata": { "href": format!("/api/v1/opendata/{scope}/metadata") },
        "daily": { "href": format!("/api/v1/opendata/{scope}/daily") },
        "monthly": { "href": format!("/api/v1/opendata/{scope}/monthly") },
    })
}

/// A JSON Schema (draft-07) document describing one payload shape.
fn schema(title: &str, schema: Value) -> Value {
    json!({
        "title": title,
        "$schema": "http://json-schema.org/draft-07/schema#",
        "schema": schema,
    })
}

fn measurement_schema() -> Value {
    schema(
        "Measurement record",
        json!({
            "type": "object",
            "required": ["station_id", "channel_id", "channel_name", "timestamp", "value", "resolution_seconds"],
            "properties": {
                "station_id": { "type": "string", "format": "uuid", "description": "Internal counting-station UUID" },
                "channel_id": { "type": "string", "format": "uuid", "description": "Channel UUID" },
                "channel_name": { "type": "string" },
                "timestamp": { "type": "string", "pattern": "^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}$", "description": "Naive central-European local time (Europe/Berlin), no UTC offset" },
                "value": { "type": "integer", "description": "Bicycle count" },
                "resolution_seconds": { "type": "integer", "description": "Length of the counted interval in seconds" }
            }
        }),
    )
}

fn distribution_schema() -> Value {
    schema(
        "Distribution",
        json!({
            "type": "object",
            "required": ["format", "url", "size_bytes", "sha256"],
            "properties": {
                "format": { "type": "string", "enum": ["parquet", "csv.gz", "json"] },
                "url": { "type": "string", "description": "Relative URL of the immutable file" },
                "size_bytes": { "type": "integer" },
                "sha256": { "type": "string", "description": "SHA-256 hex digest (also the ETag)" }
            }
        }),
    )
}

fn station_schema() -> Value {
    schema(
        "Station",
        json!({
            "type": "object",
            "required": ["station_id", "name", "description", "external_datasource_id", "timezone", "coordinates", "status"],
            "properties": {
                "station_id": { "type": "string", "format": "uuid" },
                "name": { "type": "string" },
                "description": { "type": "string" },
                "external_datasource_id": { "type": ["string", "null"], "description": "Provider-native station id (note field)" },
                "timezone": { "type": "string" },
                "coordinates": {
                    "type": ["object", "null"],
                    "properties": { "latitude": { "type": "number" }, "longitude": { "type": "number" } }
                },
                "status": { "type": "string", "enum": ["active", "inactive"] }
            }
        }),
    )
}

/// The shared dataset metadata (incl. the JSON schemata).
fn metadata_value() -> Value {
    json!({
        "name": "Bike Counter Measurements",
        "description": "Processed, immutable bicycle counting measurements of the connected German cities. Files are append-only and published once a day by the opendata_export job.",
        "timezone": TIMEZONE,
        "timestamp_format": "Naive central-European local time (Europe/Berlin), no UTC offset, e.g. 2026-09-05T14:00:00",
        "granularities": ["daily", "monthly"],
        "formats": ["parquet", "csv.gz", "json"],
        "schemas": {
            "station": station_schema(),
            "distribution": distribution_schema(),
            "measurement": measurement_schema(),
        }
    })
}

/// The distinct `YYYY` years of a (newest-first) period list.
fn available_years(periods: &[String]) -> Vec<String> {
    let mut years: Vec<String> = periods
        .iter()
        .filter_map(|period| period.split('-').next().map(ToString::to_string))
        .collect();
    years.dedup();
    years
}

/// Daily `{ date, distributions }` entries built from a period's files.
fn daily_files(files: &[OpenDataFile]) -> Vec<Value> {
    let mut by_period: HashMap<String, Vec<Value>> = HashMap::new();
    for file in files {
        by_period
            .entry(file.period.clone())
            .or_default()
            .push(distribution(file));
    }
    let mut periods: Vec<&String> = by_period.keys().collect();
    periods.sort();
    periods
        .into_iter()
        .map(|period| {
            json!({
                "date": period,
                "distributions": by_period.get(period).cloned().unwrap_or_default(),
            })
        })
        .collect()
}

// -- JSON endpoints ----------------------------------------------------------

#[utoipa::path(
    get,
    path = "/api/v1/opendata",
    tag = "OpenData",
    responses(
        (
            status = 200,
            description = "OpenData root with HATEOAS links",
            content_type = "application/json",
            example = json!({
                "_links": {
                    "self": { "href": "/api/v1/opendata" },
                    "metadata": { "href": "/api/v1/opendata/metadata" },
                    "stations": { "href": "/api/v1/opendata/stations" },
                    "stations_geojson": { "href": "/api/v1/opendata/stations.geojson" },
                    "measurements": { "href": "/api/v1/opendata/measurements" }
                }
            })
        )
    )
)]
pub async fn get_opendata_root(
    headers: HeaderMap,
) -> Result<Response, (StatusCode, Json<ErrorResponseDto>)> {
    let payload = json!({
        "_links": {
            "self": { "href": "/api/v1/opendata" },
            "metadata": { "href": "/api/v1/opendata/metadata" },
            "stations": { "href": "/api/v1/opendata/stations" },
            "stations_geojson": { "href": "/api/v1/opendata/stations.geojson" },
            "measurements": { "href": "/api/v1/opendata/measurements" },
        }
    });
    Ok(cached_json(&payload, INDEX_CACHE, &headers))
}

#[utoipa::path(
    get,
    path = "/api/v1/opendata/metadata",
    tag = "OpenData",
    responses((status = 200, description = "Dataset metadata incl. the JSON schemata"))
)]
pub async fn get_opendata_metadata(
    headers: HeaderMap,
) -> Result<Response, (StatusCode, Json<ErrorResponseDto>)> {
    Ok(cached_json(&metadata_value(), INDEX_CACHE, &headers))
}

#[utoipa::path(
    get,
    path = "/api/v1/opendata/stations",
    tag = "OpenData",
    responses((status = 200, description = "JSON list of counting stations"))
)]
pub async fn list_opendata_stations(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Result<Response, (StatusCode, Json<ErrorResponseDto>)> {
    let service = state.counting_station_service.clone();
    let stations = blocking(move || service.list(None))
        .await
        .map_err(map_domain_error)?;
    let items: Vec<Value> = stations.iter().map(station_value).collect();
    Ok(cached_json(
        &json!({ "stations": items }),
        INDEX_CACHE,
        &headers,
    ))
}

#[utoipa::path(
    get,
    path = "/api/v1/opendata/stations.geojson",
    tag = "OpenData",
    responses((status = 200, description = "Counting stations as a GeoJSON FeatureCollection"))
)]
pub async fn list_opendata_stations_geojson(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Result<Response, (StatusCode, Json<ErrorResponseDto>)> {
    let service = state.counting_station_service.clone();
    let stations = blocking(move || service.list(None))
        .await
        .map_err(map_domain_error)?;
    let features: Vec<Value> = stations
        .iter()
        .filter_map(|station| {
            let coordinates = station.coordinates?;
            let mut properties = station_value(station);
            if let Value::Object(map) = &mut properties {
                map.remove("coordinates");
            }
            Some(json!({
                "type": "Feature",
                "geometry": { "type": "Point", "coordinates": [coordinates.longitude, coordinates.latitude] },
                "properties": properties,
            }))
        })
        .collect();
    Ok(cached_json(
        &json!({ "type": "FeatureCollection", "features": features }),
        INDEX_CACHE,
        &headers,
    ))
}

#[utoipa::path(
    get,
    path = "/api/v1/opendata/measurements",
    tag = "OpenData",
    responses(
        (
            status = 200,
            description = "Measurements tree links",
            content_type = "application/json",
            example = json!({
                "_links": {
                    "self": { "href": "/api/v1/opendata/measurements" },
                    "metadata": { "href": "/api/v1/opendata/measurements/metadata" },
                    "daily": { "href": "/api/v1/opendata/measurements/daily" },
                    "monthly": { "href": "/api/v1/opendata/measurements/monthly" }
                }
            })
        )
    )
)]
pub async fn get_opendata_measurements(
    headers: HeaderMap,
) -> Result<Response, (StatusCode, Json<ErrorResponseDto>)> {
    Ok(cached_json(
        &json!({ "_links": measurements_links("measurements") }),
        INDEX_CACHE,
        &headers,
    ))
}

#[utoipa::path(
    get,
    path = "/api/v1/opendata/measurements/metadata",
    tag = "OpenData",
    responses((status = 200, description = "Measurement schema + format descriptions"))
)]
pub async fn get_opendata_measurements_metadata(
    headers: HeaderMap,
) -> Result<Response, (StatusCode, Json<ErrorResponseDto>)> {
    let payload = json!({
        "description": "Immutable measurement files. Each file contains one or more measurement records of the shared schema.",
        "measurement_schema": measurement_schema(),
        "distribution_schema": distribution_schema(),
    });
    Ok(cached_json(&payload, INDEX_CACHE, &headers))
}

async fn opendata_periods(
    state: &AppState,
    granularity: Granularity,
    station_id: Option<Uuid>,
) -> Result<Vec<String>, (StatusCode, Json<ErrorResponseDto>)> {
    let service = state.opendata_service.clone();
    blocking(move || service.list_periods(granularity, station_id))
        .await
        .map_err(map_domain_error)
}

async fn opendata_period_files(
    state: &AppState,
    granularity: Granularity,
    period: &str,
    station_id: Option<Uuid>,
) -> Result<Vec<OpenDataFile>, (StatusCode, Json<ErrorResponseDto>)> {
    let service = state.opendata_service.clone();
    let period = period.to_string();
    blocking(move || service.list_files(granularity, &period, station_id))
        .await
        .map_err(map_domain_error)
}

/// Validates that a station exists (404 otherwise) and returns its registry
/// periods of one granularity.
async fn station_periods(
    state: &AppState,
    granularity: Granularity,
    station_id: Uuid,
) -> Result<Vec<String>, (StatusCode, Json<ErrorResponseDto>)> {
    let service = state.counting_station_service.clone();
    let _ = blocking(move || service.find_by_id(Id(station_id)))
        .await
        .map_err(map_domain_error)?;
    opendata_periods(state, granularity, Some(station_id)).await
}

#[utoipa::path(
    get,
    path = "/api/v1/opendata/measurements/daily",
    tag = "OpenData",
    responses((status = 200, description = "Available years"))
)]
pub async fn get_opendata_global_daily_index(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Result<Response, (StatusCode, Json<ErrorResponseDto>)> {
    let periods = opendata_periods(&state, Granularity::Daily, None).await?;
    Ok(cached_json(
        &json!({ "granularity": "daily", "years": available_years(&periods) }),
        INDEX_CACHE,
        &headers,
    ))
}

#[utoipa::path(
    get,
    path = "/api/v1/opendata/measurements/daily/{year}",
    tag = "OpenData",
    responses((status = 200, description = "Daily files of one year"))
)]
pub async fn get_opendata_global_daily_year(
    headers: HeaderMap,
    Path(year): Path<String>,
    State(state): State<AppState>,
) -> Result<Response, (StatusCode, Json<ErrorResponseDto>)> {
    let year_prefix = format!("{year}-");
    let mut all_files = Vec::new();
    let periods = opendata_periods(&state, Granularity::Daily, None)
        .await?
        .into_iter()
        .filter(|period| period.starts_with(&year_prefix))
        .collect::<Vec<_>>();
    for period in &periods {
        all_files.extend(opendata_period_files(&state, Granularity::Daily, period, None).await?);
    }
    let payload = json!({ "year": year, "files": daily_files(&all_files) });
    Ok(cached_json(&payload, INDEX_CACHE, &headers))
}

#[utoipa::path(
    get,
    path = "/api/v1/opendata/measurements/monthly",
    tag = "OpenData",
    responses((status = 200, description = "Available year_months"))
)]
pub async fn get_opendata_global_monthly_index(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Result<Response, (StatusCode, Json<ErrorResponseDto>)> {
    let periods = opendata_periods(&state, Granularity::Monthly, None).await?;
    Ok(cached_json(
        &json!({ "granularity": "monthly", "year_months": periods }),
        INDEX_CACHE,
        &headers,
    ))
}

#[utoipa::path(
    get,
    path = "/api/v1/opendata/measurements/monthly/{year_month}",
    tag = "OpenData",
    responses((status = 200, description = "Monthly files of one year_month"))
)]
pub async fn get_opendata_global_monthly_period(
    headers: HeaderMap,
    Path(year_month): Path<String>,
    State(state): State<AppState>,
) -> Result<Response, (StatusCode, Json<ErrorResponseDto>)> {
    let files = opendata_period_files(&state, Granularity::Monthly, &year_month, None).await?;
    let distributions: Vec<Value> = files.iter().map(distribution).collect();
    Ok(cached_json(
        &json!({ "year_month": year_month, "distributions": distributions }),
        INDEX_CACHE,
        &headers,
    ))
}

#[utoipa::path(
    get,
    path = "/api/v1/opendata/stations/{station_id}",
    tag = "OpenData",
    responses(
        (
            status = 200,
            description = "Station metadata + measurements links",
            content_type = "application/json",
            example = json!({
                "station_id": "11111111-1111-1111-1111-111111111111",
                "name": "Hammer Straße",
                "description": "Radzählstation an der Hammer Straße",
                "external_datasource_id": null,
                "timezone": "Europe/Berlin",
                "coordinates": { "latitude": 51.9557, "longitude": 7.6236 },
                "status": "active",
                "_links": {
                    "measurements": { "href": "/api/v1/opendata/stations/11111111-1111-1111-1111-111111111111/measurements" }
                }
            })
        )
    )
)]
pub async fn get_opendata_station(
    headers: HeaderMap,
    Path(station_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Response, (StatusCode, Json<ErrorResponseDto>)> {
    let service = state.counting_station_service.clone();
    let station = blocking(move || service.find_by_id(Id(station_id)))
        .await
        .map_err(map_domain_error)?;
    let mut body = station_value(&station);
    if let Value::Object(map) = &mut body {
        map.insert(
            "_links".to_string(),
            json!({
                "measurements": { "href": format!("/api/v1/opendata/stations/{station_id}/measurements") },
            }),
        );
    }
    Ok(cached_json(&body, INDEX_CACHE, &headers))
}

#[utoipa::path(
    get,
    path = "/api/v1/opendata/stations/{station_id}/measurements",
    tag = "OpenData",
    responses(
        (
            status = 200,
            description = "Per-station measurements tree links",
            content_type = "application/json",
            example = json!({
                "_links": {
                    "self": { "href": "/api/v1/opendata/stations/11111111-1111-1111-1111-111111111111/measurements" },
                    "metadata": { "href": "/api/v1/opendata/stations/11111111-1111-1111-1111-111111111111/measurements/metadata" },
                    "daily": { "href": "/api/v1/opendata/stations/11111111-1111-1111-1111-111111111111/measurements/daily" },
                    "monthly": { "href": "/api/v1/opendata/stations/11111111-1111-1111-1111-111111111111/measurements/monthly" }
                }
            })
        )
    )
)]
pub async fn get_opendata_station_measurements(
    headers: HeaderMap,
    Path(station_id): Path<Uuid>,
) -> Result<Response, (StatusCode, Json<ErrorResponseDto>)> {
    let scope = format!("stations/{station_id}/measurements");
    Ok(cached_json(
        &json!({ "_links": measurements_links(&scope) }),
        INDEX_CACHE,
        &headers,
    ))
}

#[utoipa::path(
    get,
    path = "/api/v1/opendata/stations/{station_id}/measurements/metadata",
    tag = "OpenData",
    responses((status = 200, description = "Per-station measurement schema"))
)]
pub async fn get_opendata_station_measurements_metadata(
    headers: HeaderMap,
) -> Result<Response, (StatusCode, Json<ErrorResponseDto>)> {
    let payload = json!({
        "measurement_schema": measurement_schema(),
        "distribution_schema": distribution_schema(),
    });
    Ok(cached_json(&payload, INDEX_CACHE, &headers))
}

#[utoipa::path(
    get,
    path = "/api/v1/opendata/stations/{station_id}/measurements/daily",
    tag = "OpenData",
    responses((status = 200, description = "Available years of one station"))
)]
pub async fn get_opendata_station_daily_index(
    headers: HeaderMap,
    Path(station_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Response, (StatusCode, Json<ErrorResponseDto>)> {
    let periods = station_periods(&state, Granularity::Daily, station_id).await?;
    Ok(cached_json(
        &json!({ "station_id": station_id, "years": available_years(&periods) }),
        INDEX_CACHE,
        &headers,
    ))
}

#[utoipa::path(
    get,
    path = "/api/v1/opendata/stations/{station_id}/measurements/daily/{year}",
    tag = "OpenData",
    responses((status = 200, description = "Daily files of one station and year"))
)]
pub async fn get_opendata_station_daily_year(
    headers: HeaderMap,
    Path((station_id, year)): Path<(Uuid, String)>,
    State(state): State<AppState>,
) -> Result<Response, (StatusCode, Json<ErrorResponseDto>)> {
    let year_prefix = format!("{year}-");
    let periods = station_periods(&state, Granularity::Daily, station_id)
        .await?
        .into_iter()
        .filter(|period| period.starts_with(&year_prefix))
        .collect::<Vec<_>>();
    let mut all_files = Vec::new();
    for period in &periods {
        all_files.extend(
            opendata_period_files(&state, Granularity::Daily, period, Some(station_id)).await?,
        );
    }
    let payload = json!({
        "station_id": station_id,
        "year": year,
        "files": daily_files(&all_files),
    });
    Ok(cached_json(&payload, INDEX_CACHE, &headers))
}

#[utoipa::path(
    get,
    path = "/api/v1/opendata/stations/{station_id}/measurements/monthly",
    tag = "OpenData",
    responses((status = 200, description = "Available year_months of one station"))
)]
pub async fn get_opendata_station_monthly_index(
    headers: HeaderMap,
    Path(station_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Response, (StatusCode, Json<ErrorResponseDto>)> {
    let periods = station_periods(&state, Granularity::Monthly, station_id).await?;
    Ok(cached_json(
        &json!({ "station_id": station_id, "year_months": periods }),
        INDEX_CACHE,
        &headers,
    ))
}

#[utoipa::path(
    get,
    path = "/api/v1/opendata/stations/{station_id}/measurements/monthly/{year_month}",
    tag = "OpenData",
    responses((status = 200, description = "Monthly files of one station and year_month"))
)]
pub async fn get_opendata_station_monthly_period(
    headers: HeaderMap,
    Path((station_id, year_month)): Path<(Uuid, String)>,
    State(state): State<AppState>,
) -> Result<Response, (StatusCode, Json<ErrorResponseDto>)> {
    let periods = station_periods(&state, Granularity::Monthly, station_id)
        .await?
        .into_iter()
        .filter(|period| period.as_str() == year_month)
        .collect::<Vec<_>>();
    let mut files = Vec::new();
    for period in &periods {
        files.extend(
            opendata_period_files(&state, Granularity::Monthly, period, Some(station_id)).await?,
        );
    }
    let distributions: Vec<Value> = files.iter().map(distribution).collect();
    Ok(cached_json(
        &json!({ "station_id": station_id, "year_month": year_month, "distributions": distributions }),
        INDEX_CACHE,
        &headers,
    ))
}

// -- File serving ------------------------------------------------------------

/// Streams one registered opendata file with ETag / Content-Length /
/// Content-Type and an immutable `Cache-Control`.
async fn serve_file(
    headers: &HeaderMap,
    file: OpenDataFile,
    state: &AppState,
) -> Result<Response, (StatusCode, Json<ErrorResponseDto>)> {
    let etag = format!("\"{}\"", file.sha256);
    if headers
        .get(IF_NONE_MATCH)
        .is_some_and(|if_none_match| if_none_match.as_bytes() == etag.as_bytes())
    {
        let mut not_modified = Response::new(Body::empty());
        *not_modified.status_mut() = StatusCode::NOT_MODIFIED;
        not_modified
            .headers_mut()
            .insert(ETAG, HeaderValue::from_str(&etag).unwrap());
        return Ok(not_modified);
    }

    let storage = state.opendata_storage.clone();
    let object_key = ObjectKey(file.object_key.clone());
    let stream = storage.get_stream(&object_key).await.map_err(|error| {
        map_domain_error(DomainError::Database(format!(
            "failed to open the opendata file: {error:?}"
        )))
    })?;

    let mut response = Response::new(Body::from_stream(stream.body));
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_str(file.format.content_type())
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    if let Ok(length) = HeaderValue::from_str(&file.byte_size.to_string()) {
        response.headers_mut().insert(CONTENT_LENGTH, length);
    }
    if let Ok(etag) = HeaderValue::from_str(&etag) {
        response.headers_mut().insert(ETAG, etag);
    }
    response.headers_mut().insert(
        CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=31536000, immutable"),
    );
    Ok(response)
}

/// Looks a file up by its deterministic object key and streams it.
async fn serve_file_by_key(
    headers: &HeaderMap,
    object_key: String,
    state: &AppState,
) -> Result<Response, (StatusCode, Json<ErrorResponseDto>)> {
    let service = state.opendata_service.clone();
    let missing = format!("opendata file '{object_key}' not found");
    let file = blocking(move || service.find_file(&object_key))
        .await
        .map_err(map_domain_error)?
        .ok_or_else(|| not_found(missing))?;
    serve_file(headers, file, state).await
}

#[utoipa::path(
    get,
    path = "/api/v1/opendata/measurements/daily/{year}/{file}",
    tag = "OpenData",
    responses((status = 200, description = "The immutable daily file (etag-addressable)"))
)]
pub async fn get_opendata_global_daily_file(
    headers: HeaderMap,
    Path((year, file)): Path<(String, String)>,
    State(state): State<AppState>,
) -> Result<Response, (StatusCode, Json<ErrorResponseDto>)> {
    serve_file_by_key(
        &headers,
        format!("opendata/measurements/daily/{year}/{file}"),
        &state,
    )
    .await
}

#[utoipa::path(
    get,
    path = "/api/v1/opendata/measurements/monthly/{year_month}/{file}",
    tag = "OpenData",
    responses((status = 200, description = "The immutable monthly file (etag-addressable)"))
)]
pub async fn get_opendata_global_monthly_file(
    headers: HeaderMap,
    Path((year_month, file)): Path<(String, String)>,
    State(state): State<AppState>,
) -> Result<Response, (StatusCode, Json<ErrorResponseDto>)> {
    serve_file_by_key(
        &headers,
        format!("opendata/measurements/monthly/{year_month}/{file}"),
        &state,
    )
    .await
}

#[utoipa::path(
    get,
    path = "/api/v1/opendata/stations/{station_id}/measurements/daily/{year}/{file}",
    tag = "OpenData",
    responses((status = 200, description = "The immutable station daily file (etag-addressable)"))
)]
pub async fn get_opendata_station_daily_file(
    headers: HeaderMap,
    Path((station_id, year, file)): Path<(Uuid, String, String)>,
    State(state): State<AppState>,
) -> Result<Response, (StatusCode, Json<ErrorResponseDto>)> {
    serve_file_by_key(
        &headers,
        format!("opendata/stations/{station_id}/measurements/daily/{year}/{file}"),
        &state,
    )
    .await
}

#[utoipa::path(
    get,
    path = "/api/v1/opendata/stations/{station_id}/measurements/monthly/{year_month}/{file}",
    tag = "OpenData",
    responses((status = 200, description = "The immutable station monthly file (etag-addressable)"))
)]
pub async fn get_opendata_station_monthly_file(
    headers: HeaderMap,
    Path((station_id, year_month, file)): Path<(Uuid, String, String)>,
    State(state): State<AppState>,
) -> Result<Response, (StatusCode, Json<ErrorResponseDto>)> {
    serve_file_by_key(
        &headers,
        format!("opendata/stations/{station_id}/measurements/monthly/{year_month}/{file}"),
        &state,
    )
    .await
}
