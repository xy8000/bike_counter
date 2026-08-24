//! Business domain module for measurements.
//!
//! - Model: [`measurement::Measurement`] (+ `value_objects`).
//! - Driven port: [`repository_port::MeasurementRepository`] (implemented by
//!   `PostgresMeasurementRepository`).
//! - Driving port: [`service_port::MeasurementServicePort`] (implemented by
//!   `MeasurementService`).

pub mod measurement;
pub mod repository_port;
pub mod service_port;
