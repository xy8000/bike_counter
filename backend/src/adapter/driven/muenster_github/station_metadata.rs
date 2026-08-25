//! Hardcoded station metadata for the Münster counting stations.
//!
//! The Münster archive's `site_min.json` does not carry GPS coordinates, so this
//! module hardcodes the canonical name and WGS84 coordinates for the known
//! stations, keyed by their external id. Stations that are not listed here get
//! "not provided" (`None`) coordinates — they can be patched later through the
//! counting-stations API.

use crate::core::domain::data_source::provider_port::CountingStationRecord;

/// Canonical name and GPS coordinates of a Münster counting station.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StationMetadata {
    pub name: &'static str,
    pub latitude: f64,
    pub longitude: f64,
}

/// Hardcoded `external_id -> (canonical name, latitude, longitude)` for the
/// Münster counting stations (WGS84 decimal degrees).
const STATIONS: &[(&str, &str, f64, f64)] = &[
    ("300038855", "Bismarckallee", 51.9560, 7.6186),
    ("300037926", "Bohlweg", 51.9688, 7.6435),
    ("300039328", "Coesfelder Kreuz", 51.9659, 7.6012),
    ("100034978", "Gartenstraße", 51.9715, 7.6356),
    ("300037931", "Gasselstiege", 51.9796, 7.6065),
    ("300037925", "Goldstraße", 51.9680, 7.6380),
    ("300039331", "Grevener Straße", 51.9750, 7.6120),
    ("100031300", "Hafenstraße", 51.9549, 7.6289),
    ("100034980", "Hammer Straße", 51.9545, 7.6263),
    ("100034982", "Hüfferstraße", 51.9619, 7.6109),
    (
        "300037544",
        "Kanalpromenade Abschnitt 1 (Dingstiege)",
        51.9795,
        7.6632,
    ),
    ("100053305", "Kanalpromenade Abschnitt 5", 51.9181, 7.6497),
    ("300037936", "Kanalpromenade Abschnitt 6", 51.8895, 7.6500),
    ("300037928", "Kinderhauser Str.", 51.9790, 7.6210),
    ("300037920", "Lütkenbecker Weg", 51.9430, 7.6485),
    ("100035541", "Neutor", 51.9670, 7.6154),
    (
        "100031297",
        "Promenade (nördlich Salzstraße)",
        51.9606,
        7.6340,
    ),
    ("300037405", "Promenade (westlicher Hals)", 51.9589, 7.6195),
    ("300037932", "Schmeddingstraße", 51.9550, 7.5950),
    ("100034983", "Warendorfer Straße", 51.9619, 7.6375),
    ("300037933", "Weißenburg Str.", 51.9475, 7.6295),
    ("100034981", "Weseler Straße", 51.9506, 7.6177),
    ("100020113", "Wolbecker Straße", 51.9570, 7.6400),
];

/// Looks up the metadata for a station external id. `None` when the station is
/// not part of the known Münster stations (coordinates "not provided").
pub fn metadata_for(external_id: &str) -> Option<StationMetadata> {
    STATIONS
        .iter()
        .find(|(id, ..)| *id == external_id)
        .map(|(_, name, latitude, longitude)| StationMetadata {
            name,
            latitude: *latitude,
            longitude: *longitude,
        })
}

/// Overlays the hardcoded canonical name + coordinates onto a parsed station
/// record. Stations without an entry keep their archive name and get `None`
/// coordinates.
pub fn overlay(station: &mut CountingStationRecord) {
    if let Some(metadata) = metadata_for(&station.external_id) {
        station.name = metadata.name.to_string();
        station.latitude = Some(metadata.latitude);
        station.longitude = Some(metadata.longitude);
    }
}

#[cfg(test)]
mod tests {
    use super::{metadata_for, overlay};
    use crate::core::domain::data_source::provider_port::CountingStationRecord;

    #[test]
    fn metadata_for_known_station_returns_name_and_coordinates() {
        let metadata = metadata_for("300037926").expect("Bohlweg must be listed");
        assert_eq!(metadata.name, "Bohlweg");
        assert_eq!(metadata.latitude, 51.9688);
        assert_eq!(metadata.longitude, 7.6435);
    }

    #[test]
    fn metadata_for_unknown_station_is_none() {
        assert!(metadata_for("999999999").is_none());
    }

    #[test]
    fn overlay_applies_name_and_coordinates_to_listed_station() {
        let mut station = CountingStationRecord {
            external_id: "100031297".to_string(),
            name: "Promenade (nördl. Salzstraße)".to_string(),
            description: String::new(),
            latitude: None,
            longitude: None,
            timezone: "Europe/Berlin".to_string(),
            image_sha256: None,
        };
        overlay(&mut station);
        assert_eq!(station.name, "Promenade (nördlich Salzstraße)");
        assert_eq!(station.latitude, Some(51.9606));
        assert_eq!(station.longitude, Some(7.6340));
    }

    #[test]
    fn overlay_leaves_unlisted_station_untouched() {
        let mut station = CountingStationRecord {
            external_id: "424242".to_string(),
            name: "Unlisted Station".to_string(),
            description: String::new(),
            latitude: None,
            longitude: None,
            timezone: "Europe/Berlin".to_string(),
            image_sha256: None,
        };
        overlay(&mut station);
        assert_eq!(station.name, "Unlisted Station");
        assert_eq!(station.latitude, None);
        assert_eq!(station.longitude, None);
    }
}
