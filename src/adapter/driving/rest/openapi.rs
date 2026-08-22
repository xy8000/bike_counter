use utoipa::OpenApi;

use crate::adapter::driving::rest::dto::{
    ApiRootDto, ChannelDto, ChannelListDto, CountingStationDto,
    CountingStationListDto, ErrorResponseDto, LinkDto, MeasurementDto, MeasurementListDto,
};
use crate::adapter::driving::rest::handlers::{
    __path_get_api_root, __path_get_channel_by_id, __path_get_counting_station_by_id,
    __path_get_measurement_by_id, __path_list_channels, __path_list_counting_stations,
    __path_list_measurements,
};

#[derive(OpenApi)]
#[openapi(
    paths(
        get_api_root,
        list_counting_stations,
        get_counting_station_by_id,
        list_channels,
        get_channel_by_id,
        list_measurements,
        get_measurement_by_id,
    ),
    components(
        schemas(
            ApiRootDto,
            CountingStationDto,
            CountingStationListDto,
            ChannelDto,
            ChannelListDto,
            MeasurementDto,
            MeasurementListDto,
            LinkDto,
            ErrorResponseDto,
        )
    ),
    tags(
        (name = "Root", description = "Root discovery endpoint"),
        (name = "Counting Stations", description = "Operations on bike counting stations"),
        (name = "Channels", description = "Operations on counting station channels"),
        (name = "Measurements", description = "Operations on channel measurements"),
    ),
    info(
        title = "Bike Counter REST API",
        version = "1.0.0",
        description = "Read-Only RESTful API with HATEOAS links and flat URL hierarchy for Bike Counter Stations"
    )
)]
pub struct ApiDoc;
