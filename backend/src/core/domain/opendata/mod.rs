//! OpenData export domain: the immutable, append-only published-file registry
//! plus the ports that produce and read the published data.
//!
//! - [`file`] — the [`OpenDataFile`] aggregate and its [`Granularity`]/[`Format`]
//!   value objects.
//! - [`measurement`] — the export row ([`OpenDataMeasurement`]) that the core
//!   hands to a file-generator adapter.
//! - [`file_repository_port`] — driven DB port for the registry (the job's state).
//! - [`measurement_reader_port`] — driven DB port returning export rows and the
//!   distinct periods that actually contain measurements.
//! - [`file_generator_port`] — driven port the adapter implements to serialize
//!   measurements into parquet / csv.gz / json bytes.
//! - [`service_port`] — driving port the REST handlers consume.

pub mod file;
pub mod file_generator_port;
pub mod file_repository_port;
pub mod measurement;
pub mod measurement_reader_port;
pub mod service_port;
