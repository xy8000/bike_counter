use utoipa::OpenApi;

use crate::adapter::driving::rest::dto::{
    ApiRootDto, ChannelDto, ChannelListDto, CountingStationDto, CountingStationListDto,
    DataSourceDto, DataSourceListDto, ErrorResponseDto, HealthComponentDto, HealthDto, LinkDto,
    MeasurementDto, MeasurementListDto,
};
use crate::adapter::driving::rest::handlers::{
    __path_get_api_root, __path_get_channel_by_id, __path_get_counting_station_by_id,
    __path_get_data_source_by_id, __path_get_health_live, __path_get_health_ready,
    __path_get_measurement_by_id, __path_list_channels, __path_list_counting_stations,
    __path_list_data_sources, __path_list_measurements,
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
        list_data_sources,
        get_data_source_by_id,
        get_health_live,
        get_health_ready,
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
            DataSourceDto,
            DataSourceListDto,
            LinkDto,
            ErrorResponseDto,
            HealthComponentDto,
            HealthDto,
        )
    ),
    tags(
        (name = "Root", description = "Root discovery endpoint"),
        (name = "Counting Stations", description = "Operations on bike counting stations"),
        (name = "Channels", description = "Operations on counting station channels"),
        (name = "Measurements", description = "Operations on channel measurements"),
        (name = "Data Sources", description = "External data sources"),
        (name = "Health", description = "Operational health checks (liveness/readiness)"),
    ),
    info(
        title = "Bike Counter REST API",
        version = "1.0.0",
        description = "Read-Only RESTful API with HATEOAS links and flat URL hierarchy for Bike Counter Stations"
    )
)]
pub struct ApiDoc;
