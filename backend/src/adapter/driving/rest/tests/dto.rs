//! Structural unit checks for the HATEOAS DTOs.

use crate::adapter::driving::rest::dto::{
    ChannelDto, ChannelListDto, CountingStationDto, CountingStationListDto, JobDto, JobListDto,
    JobStatusDto, LinkDto, MeasurementDto, MeasurementListDto, RawMeasurementDto,
};
use crate::adapter::driving::rest::tests::fixtures::{
    CHANNEL_ID_A, DATA_SOURCE_ID_A, JOB_ID_A, MEASUREMENT_ID_A, STATION_ID_A, channel_a, job_a,
    job_b, measurement_a, station_a, station_linked_to_data_source_a, timestamp,
};
use crate::core::domain::jobs::job::JobStatus;

#[test]
fn counting_station_dto_contains_expected_links() {
    let dto = CountingStationDto::from(station_a());

    assert_eq!(
        dto.links["self"].href,
        format!("/api/v1/counting-stations/{STATION_ID_A}")
    );
    assert_eq!(
        dto.links["channels"].href,
        format!("/api/v1/channels?counting_station_id={STATION_ID_A}")
    );
    assert_eq!(dto.links["collection"].href, "/api/v1/counting-stations");
    // Every counting station was imported from a data source, so the id and the
    // `data_source` link are always present.
    assert_eq!(dto.data_source_id, DATA_SOURCE_ID_A);
    assert_eq!(dto.latitude, Some(51.9565));
    assert_eq!(dto.longitude, Some(7.6152));
    assert_eq!(
        dto.links["data_source"].href,
        format!("/api/v1/data-sources/{DATA_SOURCE_ID_A}")
    );
}

#[test]
fn counting_station_dto_exposes_data_source_id_and_link_when_linked() {
    let dto = CountingStationDto::from(station_linked_to_data_source_a());

    assert_eq!(dto.data_source_id, DATA_SOURCE_ID_A);
    // This fixture has no coordinates: the DTO fields are "not provided".
    assert_eq!(dto.latitude, None);
    assert_eq!(dto.longitude, None);
    assert_eq!(
        dto.links["data_source"].href,
        format!("/api/v1/data-sources/{DATA_SOURCE_ID_A}")
    );
}

#[test]
fn channel_dto_contains_expected_links() {
    let dto = ChannelDto::from(channel_a());

    assert_eq!(
        dto.links["self"].href,
        format!("/api/v1/channels/{CHANNEL_ID_A}")
    );
    assert_eq!(
        dto.links["counting_station"].href,
        format!("/api/v1/counting-stations/{STATION_ID_A}")
    );
    assert_eq!(
        dto.links["measurements"].href,
        format!("/api/v1/measurements?channel_id={CHANNEL_ID_A}")
    );
}

#[test]
fn measurement_dto_contains_expected_links() {
    let dto = MeasurementDto::from(measurement_a());

    assert_eq!(
        dto.links["self"].href,
        format!("/api/v1/measurements/{MEASUREMENT_ID_A}")
    );
    assert_eq!(
        dto.links["channel"].href,
        format!("/api/v1/channels/{CHANNEL_ID_A}")
    );
}

#[test]
fn raw_measurement_dto_maps_fields_without_links() {
    let dto = RawMeasurementDto::from(measurement_a());

    assert_eq!(dto.id, MEASUREMENT_ID_A);
    assert_eq!(dto.channel_id, CHANNEL_ID_A);
    assert_eq!(dto.value, 42);
    assert_eq!(dto.timestamp, timestamp());
}

#[test]
fn list_dtos_build_consistent_self_links() {
    let station_filter = Some(STATION_ID_A);

    assert_eq!(
        ChannelListDto::new(vec![], station_filter, None).links["self"].href,
        format!("/api/v1/channels?counting_station_id={STATION_ID_A}")
    );
    assert_eq!(
        MeasurementListDto::new(vec![], station_filter, 0, 100, false).links["self"].href,
        format!("/api/v1/measurements?channel_id={STATION_ID_A}&offset=0&limit=100")
    );
    assert_eq!(
        CountingStationListDto::new(vec![], None).links["self"].href,
        "/api/v1/counting-stations"
    );
}

#[test]
fn link_dto_serializes_as_object_with_href() {
    let json = serde_json::to_value(LinkDto::new("/api/v1")).expect("link should serialize");
    assert_eq!(json["href"], "/api/v1");
    // A concrete link must not carry the HAL `templated` flag.
    assert!(json.get("templated").is_none());
}

#[test]
fn templated_link_serializes_with_templated_flag() {
    let json = serde_json::to_value(LinkDto::templated(
        "/api/v1/data-sources/{id}/persistent_state/{key}",
    ))
    .expect("link should serialize");
    assert_eq!(
        json["href"],
        "/api/v1/data-sources/{id}/persistent_state/{key}"
    );
    assert_eq!(json["templated"], true);
}

#[test]
fn job_status_dto_maps_all_domain_statuses() {
    assert_eq!(
        JobStatusDto::from(JobStatus::Pending),
        JobStatusDto::Pending
    );
    assert_eq!(
        JobStatusDto::from(JobStatus::Running),
        JobStatusDto::Running
    );
    assert_eq!(
        JobStatusDto::from(JobStatus::Finished),
        JobStatusDto::Finished
    );
    assert_eq!(JobStatusDto::from(JobStatus::Failed), JobStatusDto::Failed);
}

#[test]
fn job_dto_maps_fields_and_links() {
    let dto = JobDto::from(job_a());

    assert_eq!(dto.id, JOB_ID_A);
    assert_eq!(dto.job_type, "data_source_update");
    assert_eq!(dto.status, JobStatusDto::Finished);
    assert_eq!(dto.links["self"].href, format!("/api/v1/jobs/{JOB_ID_A}"));
    assert_eq!(dto.links["collection"].href, "/api/v1/jobs");
    assert_eq!(dto.links["root"].href, "/api/v1");
}

#[test]
fn job_list_dto_builds_items_and_links() {
    let list = JobListDto::new(vec![job_a(), job_b()]);

    assert_eq!(list.items.len(), 2);
    assert_eq!(list.items[0].status, JobStatusDto::Finished);
    assert_eq!(list.items[1].status, JobStatusDto::Running);
    assert_eq!(list.links["self"].href, "/api/v1/jobs");
    assert_eq!(list.links["root"].href, "/api/v1");
}
