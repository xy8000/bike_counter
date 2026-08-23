use utoipa::OpenApi;

use crate::adapter::driving::rest::dto::{
    ApiRootDto, ChannelDto, ChannelListDto, CountingStationDto, CountingStationListDto,
    DataSourceDto, DataSourceListDto, ErrorResponseDto, HealthComponentDto, HealthDto, JobDto,
    JobListDto, JobQueryParams, JobStatusDto, LinkDto, MeasurementDto, MeasurementListDto,
    PersistentStateDto, PersistentStateEntryDto, PersistentStateValueDto,
};
use crate::adapter::driving::rest::handlers::{
    __path_clear_persistent_state, __path_delete_persistent_state_entry, __path_get_api_root,
    __path_get_channel_by_id, __path_get_counting_station_by_id, __path_get_data_source_by_id,
    __path_get_health_live, __path_get_health_ready, __path_get_job_by_id,
    __path_get_measurement_by_id, __path_get_persistent_state, __path_list_channels,
    __path_list_counting_stations, __path_list_data_sources, __path_list_jobs,
    __path_list_measurements, __path_put_persistent_state_entry,
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
        get_persistent_state,
        put_persistent_state_entry,
        delete_persistent_state_entry,
        clear_persistent_state,
        list_jobs,
        get_job_by_id,
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
            JobDto,
            JobListDto,
            JobStatusDto,
            JobQueryParams,
            PersistentStateDto,
            PersistentStateEntryDto,
            PersistentStateValueDto,
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
        (name = "Jobs", description = "Generic tracked jobs"),
        (name = "Health", description = "Operational health checks (liveness/readiness)"),
    ),
    info(
        title = "Bike Counter REST API",
        version = "1.0.0",
        description = "RESTful API with HATEOAS links and flat URL hierarchy for Bike Counter Stations"
    )
)]
pub struct ApiDoc;
