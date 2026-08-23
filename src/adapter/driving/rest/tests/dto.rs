//! Structural unit checks for the HATEOAS DTOs.

use crate::adapter::driving::rest::dto::{
    ChannelDto, ChannelListDto, CountingStationDto, CountingStationListDto, LinkDto,
    MeasurementDto, MeasurementListDto,
};
use crate::adapter::driving::rest::tests::fixtures::{
    CHANNEL_ID_A, MEASUREMENT_ID_A, STATION_ID_A, channel_a, measurement_a, station_a,
};

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
