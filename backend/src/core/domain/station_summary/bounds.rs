//! Geographic bounding-box value object owned by the station-summary domain,
//! used to filter the visible stations by the current map viewport.

use crate::core::domain::counting_stations::counting_station::value_objects::GeoCoordinates;

/// A geographic bounding box (WGS84 decimal degrees) used to filter counting
/// stations by the currently visible map viewport.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeoBounds {
    pub min_latitude: f64,
    pub min_longitude: f64,
    pub max_latitude: f64,
    pub max_longitude: f64,
}

impl GeoBounds {
    /// Returns `true` when the box is well-formed, i.e. every axis spans a
    /// non-negative range (`min <= max`).
    pub fn is_valid(&self) -> bool {
        self.min_latitude <= self.max_latitude && self.min_longitude <= self.max_longitude
    }

    /// Returns `true` when `coordinates` lie inside the box (inclusive bounds).
    pub fn contains(&self, coordinates: GeoCoordinates) -> bool {
        coordinates.latitude >= self.min_latitude
            && coordinates.latitude <= self.max_latitude
            && coordinates.longitude >= self.min_longitude
            && coordinates.longitude <= self.max_longitude
    }
}
