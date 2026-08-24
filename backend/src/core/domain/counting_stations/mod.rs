//! Business domain module for counting stations.
//!
//! - Model: [`counting_station::CountingStation`] (+ `value_objects`).
//! - Driven port: [`repository_port::CountingStationRepository`] (implemented by
//!   `PostgresCountingStationRepository`).
//! - Driving port: [`service_port::CountingStationServicePort`] (implemented by
//!   `CountingStationService`).

pub mod counting_station;
pub mod repository_port;
pub mod service_port;
