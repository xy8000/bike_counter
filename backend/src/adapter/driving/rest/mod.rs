//! Driving REST adapter: the public read API, the BFF used by the SPA and the
//! token-guarded admin mutations, all behind one shared HTTP policy.
//!
//! ## Layer stack
//!
//! The router is split into sub-routers so each surface can carry its own
//! timeout, then merged and wrapped once by the shared policy. Layers are added
//! outermost-first: the request-id layer must run **outside** the trace layer,
//! otherwise the span has no id yet (the header assertion tests cover this).
//!
//! ```text
//! request-id (generate + propagate)
//!   └─ trace span (method, path, request id) and one completion event
//!        └─ catch-panic (handler panics become a 500 JSON body)
//!             └─ 1 MiB request body limit
//!                  └─ global concurrency limit (bounds queued blocking work)
//!                       ├─ BFF routes (15 s)
//!                       ├─ REST read routes (30 s)
//!                       ├─ OpenData file routes (60 s)
//!                       └─ Admin mutations (bearer auth, 30 s) - only when
//!                          `admin_token` is configured
//! ```
//!
//! A timeout bounds the **response**, not the `spawn_blocking` work already
//! queued; the concurrency limit is what stops that work from growing
//! unboundedly.

pub mod admin_auth;
pub mod dto;
pub mod handlers;
pub mod openapi;

#[cfg(test)]
mod tests;

use std::any::Any;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::{DefaultBodyLimit, Request};
use axum::http::{HeaderMap, HeaderName, StatusCode};
use axum::middleware::from_fn_with_state;
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{delete, get, patch, post, put};
use tower::limit::ConcurrencyLimitLayer;
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::adapter::driving::bff::{
    get_bff_asset_content, get_bff_data_source_detail, get_bff_data_source_imports,
    get_bff_data_sources, get_bff_global_summary, get_bff_station_detail_graphs,
    get_bff_station_detail_monthly, get_bff_station_detail_overview, get_bff_station_detail_page,
    get_bff_station_overview, get_bff_station_overview_stats, get_bff_stations_search,
    get_bff_stations_sidebar, get_bff_stations_sidebar_stats, get_bff_stations_summary_graphs,
    get_bff_stations_summary_monthly, get_bff_stations_summary_overview,
    get_bff_stations_summary_page, get_bff_version, list_bff_stations,
};
use crate::adapter::driving::rest::admin_auth::{AdminAuth, require_admin};
use crate::adapter::driving::rest::dto::ErrorResponseDto;
pub use crate::adapter::driving::rest::handlers::AppState;
use crate::adapter::driving::rest::handlers::{
    cancel_job, clear_persistent_state, delete_persistent_state_entry, get_api_root,
    get_channel_by_id, get_counting_station_by_id, get_data_source_by_id, get_health_live,
    get_health_providers, get_health_ready, get_job_by_id, get_measurement_by_id,
    get_opendata_global_daily_file, get_opendata_global_daily_index,
    get_opendata_global_daily_year, get_opendata_global_monthly_file,
    get_opendata_global_monthly_index, get_opendata_global_monthly_period,
    get_opendata_measurements, get_opendata_measurements_metadata, get_opendata_metadata,
    get_opendata_root, get_opendata_station, get_opendata_station_daily_file,
    get_opendata_station_daily_index, get_opendata_station_daily_year,
    get_opendata_station_measurements, get_opendata_station_measurements_metadata,
    get_opendata_station_monthly_file, get_opendata_station_monthly_index,
    get_opendata_station_monthly_period, get_persistent_state, list_channels,
    list_counting_stations, list_data_sources, list_jobs, list_measurements, list_measurements_raw,
    list_opendata_stations, list_opendata_stations_geojson, list_provider_messages,
    patch_counting_station, put_persistent_state_entry, reset_imported_until,
};
use crate::adapter::driving::rest::openapi::ApiDoc;
use crate::core::application::provider_health_cache::ProviderHealthCache;
use crate::core::domain::assets::asset_storage_port::AssetStorage;
use crate::core::domain::assets::service_port::AssetServicePort;
use crate::core::domain::channels::service_port::ChannelServicePort;
use crate::core::domain::counting_stations::service_port::CountingStationServicePort;
use crate::core::domain::data_source::service_port::DataSourceServicePort;
use crate::core::domain::data_source::service_port::PersistentStateServicePort;
use crate::core::domain::data_source::service_port::ProviderMessageServicePort;
use crate::core::domain::data_source_analytics::DataSourceAnalyticsServicePort;
use crate::core::domain::health::service_port::HealthServicePort;
use crate::core::domain::jobs::service_port::JobServicePort;
use crate::core::domain::measurements::service_port::MeasurementServicePort;
use crate::core::domain::opendata::service_port::OpenDataServicePort;
use crate::core::domain::station_analytics::service_port::StationAnalyticsServicePort;

/// Request-id header generated when absent and echoed on every response.
pub const REQUEST_ID_HEADER: &str = "x-request-id";

/// Largest accepted request body: the API only takes small admin JSON payloads,
/// so anything above this is abuse rather than a legitimate request.
const REQUEST_BODY_LIMIT_BYTES: usize = 1024 * 1024;

/// BFF timeout: the SPA's page and card requests are interactive.
const BFF_TIMEOUT: Duration = Duration::from_secs(15);
/// Public REST read timeout.
const REST_TIMEOUT: Duration = Duration::from_secs(30);
/// Admin mutation timeout (same budget as the REST reads).
const ADMIN_TIMEOUT: Duration = Duration::from_secs(30);
/// OpenData file timeout: a file response can stream for longer than JSON.
const OPENDATA_TIMEOUT: Duration = Duration::from_secs(60);

/// Per-deployment HTTP policy for the router.
///
/// `admin_token` is `None` when the data-mutating routes must not exist at all
/// (they answer 404); `max_concurrent_requests` is the global edge limit that
/// keeps queued work from occupying every blocking-pool thread.
#[derive(Clone)]
pub struct RestApiConfig {
    admin_token: Option<Arc<str>>,
    max_concurrent_requests: usize,
}

/// Concurrency limit used by [`RestApiConfig::read_only`].
pub const DEFAULT_MAX_CONCURRENT_REQUESTS: usize = 20;

impl RestApiConfig {
    pub fn new(admin_token: Option<Arc<str>>, max_concurrent_requests: usize) -> Self {
        Self {
            admin_token,
            max_concurrent_requests,
        }
    }

    /// Read-only surface: no admin routes, default concurrency limit.
    pub fn read_only() -> Self {
        Self {
            admin_token: None,
            max_concurrent_requests: DEFAULT_MAX_CONCURRENT_REQUESTS,
        }
    }

    /// The configured admin token, if any.
    pub fn admin_token(&self) -> Option<&str> {
        self.admin_token.as_deref()
    }

    /// The global concurrency limit.
    pub fn max_concurrent_requests(&self) -> usize {
        self.max_concurrent_requests
    }
}

impl Default for RestApiConfig {
    fn default() -> Self {
        Self::read_only()
    }
}

pub struct RestApiAdapter {
    app_state: AppState,
    config: RestApiConfig,
}

impl RestApiAdapter {
    /// Pure dependency wiring: the adapter takes every backing core service.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        counting_station_service: Arc<dyn CountingStationServicePort + Send + Sync>,
        channel_service: Arc<dyn ChannelServicePort + Send + Sync>,
        measurement_service: Arc<dyn MeasurementServicePort + Send + Sync>,
        data_source_service: Arc<dyn DataSourceServicePort + Send + Sync>,
        job_service: Arc<dyn JobServicePort + Send + Sync>,
        health_service: Arc<dyn HealthServicePort + Send + Sync>,
        provider_health: Arc<ProviderHealthCache>,
        persistent_state_service: Arc<dyn PersistentStateServicePort + Send + Sync>,
        provider_message_service: Arc<dyn ProviderMessageServicePort + Send + Sync>,
        station_analytics_service: Arc<dyn StationAnalyticsServicePort + Send + Sync>,
        data_source_analytics_service: Arc<dyn DataSourceAnalyticsServicePort + Send + Sync>,
        asset_service: Arc<dyn AssetServicePort>,
        asset_storage: Arc<dyn AssetStorage>,
        opendata_service: Arc<dyn OpenDataServicePort + Send + Sync>,
        opendata_storage: Arc<dyn AssetStorage>,
        config: RestApiConfig,
    ) -> Self {
        Self {
            app_state: AppState {
                counting_station_service,
                channel_service,
                measurement_service,
                data_source_service,
                job_service,
                health_service,
                provider_health,
                persistent_state_service,
                provider_message_service,
                station_analytics_service,
                data_source_analytics_service,
                asset_service,
                asset_storage,
                opendata_service,
                opendata_storage,
            },
            config,
        }
    }

    pub fn router(&self) -> Router {
        Self::create_router_with_config(self.app_state.clone(), self.config.clone())
    }

    /// Builds the router; the admin routes exist only when
    /// [`RestApiConfig::admin_token`] is set.
    pub fn create_router_with_config(app_state: AppState, config: RestApiConfig) -> Router {
        let router: Router<AppState> = Router::new()
            .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
            .merge(bff_router())
            .merge(rest_read_router())
            .merge(opendata_router());

        let router = if let Some(token) = config.admin_token() {
            router.merge(admin_router(token))
        } else {
            // Fail closed: without a token the mutating routes do not exist.
            tracing::warn!(
                "admin_token is not configured: the data-mutating /api/v1 routes \
                 (station patch, persistent state, imported_until reset, job cancel) \
                 are disabled and answer 404"
            );
            router
        };

        // The state is applied before the layers so every surface shares one
        // policy stack and the result is a `Router<()>`.
        router
            .with_state(app_state)
            .layer(ConcurrencyLimitLayer::new(config.max_concurrent_requests()))
            .layer(DefaultBodyLimit::max(REQUEST_BODY_LIMIT_BYTES))
            .layer(CatchPanicLayer::custom(handle_panic))
            .layer(
                TraceLayer::new_for_http()
                    .make_span_with(|request: &Request| {
                        tracing::info_span!(
                            "http",
                            method = %request.method(),
                            path = %request.uri().path(),
                            request_id = %request_id_of(request.headers()),
                        )
                    })
                    .on_response(
                        |response: &Response, latency: Duration, _span: &tracing::Span| {
                            tracing::info!(
                                status = response.status().as_u16(),
                                latency_ms = u64::try_from(latency.as_millis()).unwrap_or(u64::MAX),
                                "request completed"
                            );
                        },
                    ),
            )
            .layer(PropagateRequestIdLayer::new(HeaderName::from_static(
                REQUEST_ID_HEADER,
            )))
            // Added last, so it is the outermost layer: the id exists before the
            // trace span is created and before any handler runs.
            .layer(SetRequestIdLayer::new(
                HeaderName::from_static(REQUEST_ID_HEADER),
                MakeRequestUuid,
            ))
    }

    pub async fn run(&self, addr: SocketAddr) -> Result<(), std::io::Error> {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, self.router()).await
    }
}

/// The request id set by [`SetRequestIdLayer`], or `-` when unavailable.
fn request_id_of(headers: &HeaderMap) -> &str {
    headers
        .get(REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("-")
}

/// Turns a handler panic into a `500` with the shared error body instead of a
/// dropped connection. Several helpers `expect` infallible serialization and
/// asset lookups, so panics must stay inside the process boundary.
fn handle_panic(panic: Box<dyn Any + Send + 'static>) -> Response {
    let message = if let Some(message) = panic.downcast_ref::<String>() {
        message.as_str()
    } else if let Some(message) = panic.downcast_ref::<&str>() {
        message
    } else {
        "unknown panic payload"
    };
    tracing::error!("Handler panicked: {message}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponseDto {
            error: "Internal server error".to_string(),
        }),
    )
        .into_response()
}

/// BFF routes used by the SPA (short interactive timeout).
fn bff_router() -> Router<AppState> {
    Router::new()
        .route("/api/bff/stations", get(list_bff_stations))
        .route("/api/bff/stations/sidebar", get(get_bff_stations_sidebar))
        .route(
            "/api/bff/stations/sidebar/stats",
            get(get_bff_stations_sidebar_stats),
        )
        .route("/api/bff/stations/search", get(get_bff_stations_search))
        .route("/api/bff/global-summary", get(get_bff_global_summary))
        .route("/api/bff/version", get(get_bff_version))
        .route(
            "/api/bff/station-overview/{id}",
            get(get_bff_station_overview),
        )
        .route(
            "/api/bff/station-overview/{id}/stats",
            get(get_bff_station_overview_stats),
        )
        .route(
            "/api/bff/station-detail/{id}",
            get(get_bff_station_detail_page),
        )
        .route(
            "/api/bff/station-detail/{id}/overview",
            get(get_bff_station_detail_overview),
        )
        .route(
            "/api/bff/station-detail/{id}/graphs/{timeframe}",
            get(get_bff_station_detail_graphs),
        )
        .route(
            "/api/bff/station-detail/{id}/monthly",
            get(get_bff_station_detail_monthly),
        )
        .route(
            "/api/bff/stations/summary",
            get(get_bff_stations_summary_page),
        )
        .route(
            "/api/bff/stations/summary/overview",
            get(get_bff_stations_summary_overview),
        )
        .route(
            "/api/bff/stations/summary/graphs/{timeframe}",
            get(get_bff_stations_summary_graphs),
        )
        .route(
            "/api/bff/stations/summary/monthly",
            get(get_bff_stations_summary_monthly),
        )
        .route("/api/bff/assets/{id}/content", get(get_bff_asset_content))
        .route("/api/bff/data-sources", get(get_bff_data_sources))
        .route(
            "/api/bff/data-sources/{id}",
            get(get_bff_data_source_detail),
        )
        .route(
            "/api/bff/data-sources/{id}/imports",
            get(get_bff_data_source_imports),
        )
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            BFF_TIMEOUT,
        ))
}

/// Anonymous `/api/v1` reads plus the health endpoints.
fn rest_read_router() -> Router<AppState> {
    Router::new()
        .route("/api/v1", get(get_api_root))
        .route("/api/v1/counting-stations", get(list_counting_stations))
        .route(
            "/api/v1/counting-stations/{id}",
            get(get_counting_station_by_id),
        )
        .route("/api/v1/channels", get(list_channels))
        .route("/api/v1/channels/{id}", get(get_channel_by_id))
        .route("/api/v1/measurements", get(list_measurements))
        .route("/api/v1/measurements/raw", get(list_measurements_raw))
        .route("/api/v1/measurements/{id}", get(get_measurement_by_id))
        .route("/api/v1/data-sources", get(list_data_sources))
        .route("/api/v1/data-sources/{id}", get(get_data_source_by_id))
        .route(
            "/api/v1/data-sources/{id}/persistent_state",
            get(get_persistent_state),
        )
        .route(
            "/api/v1/data-sources/{id}/messages",
            get(list_provider_messages),
        )
        .route("/api/v1/jobs", get(list_jobs))
        .route("/api/v1/jobs/{id}", get(get_job_by_id))
        .route("/health/live", get(get_health_live))
        .route("/health/ready", get(get_health_ready))
        .route("/health/providers", get(get_health_providers))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            REST_TIMEOUT,
        ))
}

/// The immutable, append-only OpenData file tree (longer timeout).
fn opendata_router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/opendata", get(get_opendata_root))
        .route("/api/v1/opendata/metadata", get(get_opendata_metadata))
        .route("/api/v1/opendata/stations", get(list_opendata_stations))
        .route(
            "/api/v1/opendata/stations.geojson",
            get(list_opendata_stations_geojson),
        )
        .route(
            "/api/v1/opendata/measurements",
            get(get_opendata_measurements),
        )
        .route(
            "/api/v1/opendata/measurements/metadata",
            get(get_opendata_measurements_metadata),
        )
        .route(
            "/api/v1/opendata/measurements/daily",
            get(get_opendata_global_daily_index),
        )
        .route(
            "/api/v1/opendata/measurements/daily/{year}",
            get(get_opendata_global_daily_year),
        )
        .route(
            "/api/v1/opendata/measurements/daily/{year}/{file}",
            get(get_opendata_global_daily_file),
        )
        .route(
            "/api/v1/opendata/measurements/monthly",
            get(get_opendata_global_monthly_index),
        )
        .route(
            "/api/v1/opendata/measurements/monthly/{year_month}",
            get(get_opendata_global_monthly_period),
        )
        .route(
            "/api/v1/opendata/measurements/monthly/{year_month}/{file}",
            get(get_opendata_global_monthly_file),
        )
        .route(
            "/api/v1/opendata/stations/{station_id}",
            get(get_opendata_station),
        )
        .route(
            "/api/v1/opendata/stations/{station_id}/measurements",
            get(get_opendata_station_measurements),
        )
        .route(
            "/api/v1/opendata/stations/{station_id}/measurements/metadata",
            get(get_opendata_station_measurements_metadata),
        )
        .route(
            "/api/v1/opendata/stations/{station_id}/measurements/daily",
            get(get_opendata_station_daily_index),
        )
        .route(
            "/api/v1/opendata/stations/{station_id}/measurements/daily/{year}",
            get(get_opendata_station_daily_year),
        )
        .route(
            "/api/v1/opendata/stations/{station_id}/measurements/daily/{year}/{file}",
            get(get_opendata_station_daily_file),
        )
        .route(
            "/api/v1/opendata/stations/{station_id}/measurements/monthly",
            get(get_opendata_station_monthly_index),
        )
        .route(
            "/api/v1/opendata/stations/{station_id}/measurements/monthly/{year_month}",
            get(get_opendata_station_monthly_period),
        )
        .route(
            "/api/v1/opendata/stations/{station_id}/measurements/monthly/{year_month}/{file}",
            get(get_opendata_station_monthly_file),
        )
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            OPENDATA_TIMEOUT,
        ))
}

/// The data-mutating routes, guarded by a bearer token. URLs are unchanged so
/// existing documentation and scripts keep working once a token is configured.
fn admin_router(token: &str) -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/counting-stations/{id}",
            patch(patch_counting_station),
        )
        .route(
            "/api/v1/data-sources/{id}/persistent_state/{key}",
            put(put_persistent_state_entry).delete(delete_persistent_state_entry),
        )
        .route(
            "/api/v1/data-sources/{id}/persistent_state",
            delete(clear_persistent_state),
        )
        .route(
            "/api/v1/data-sources/{id}/imported_until",
            delete(reset_imported_until),
        )
        .route("/api/v1/jobs/{id}/cancel", post(cancel_job))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            ADMIN_TIMEOUT,
        ))
        .route_layer(from_fn_with_state(AdminAuth::new(token), require_admin))
}
