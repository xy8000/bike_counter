use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Json;
use uuid::Uuid;

use super::{AppState, blocking, map_domain_error};
use crate::adapter::driving::rest::dto::{
    ChannelDto, ChannelListDto, ChannelQueryParams, ErrorResponseDto,
};
use crate::core::domain::channels::channel::value_objects as channel_vo;

#[utoipa::path(
    get,
    path = "/api/v1/channels",
    tag = "Channels",
    params(
        ChannelQueryParams
    ),
    responses(
        (status = 200, description = "List channels with optional counting_station_id filter and HATEOAS links", body = ChannelListDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn list_channels(
    State(state): State<AppState>,
    Query(params): Query<ChannelQueryParams>,
) -> Result<Json<ChannelListDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let service = state.channel_service.clone();
    let station_id_filter = params.counting_station_id;
    let name_filter = params.name.clone();
    let dto_name = name_filter.clone();
    let station_id = station_id_filter.map(channel_vo::CountingStationId);
    let channels = blocking(move || service.list(station_id, name_filter.as_deref()))
        .await
        .map_err(map_domain_error)?;

    Ok(Json(ChannelListDto::new(
        channels,
        station_id_filter,
        dto_name.as_deref(),
    )))
}

#[utoipa::path(
    get,
    path = "/api/v1/channels/{id}",
    tag = "Channels",
    params(
        ("id" = Uuid, Path, description = "Channel UUID")
    ),
    responses(
        (status = 200, description = "Channel found", body = ChannelDto),
        (status = 404, description = "Channel not found", body = ErrorResponseDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn get_channel_by_id(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ChannelDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let service = state.channel_service.clone();
    let channel_id = channel_vo::Id(id);
    let channel = blocking(move || service.find_by_id(channel_id))
        .await
        .map_err(map_domain_error)?;
    Ok(Json(ChannelDto::from(channel)))
}
