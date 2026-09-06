//! Driven (outbound) port the file-generator **adapter** implements: turning an
//! export row slice and a [`Format`] into the bytes of one distribution file.
//! The core decides *what* to publish and *when*; only this adapter knows how to
//! serialize a format.

use super::file::Format;
use super::measurement::OpenDataMeasurement;
use crate::core::domain::error::DomainError;

pub trait OpenDataFileGenerator: Send + Sync {
    /// Serializes `rows` into a complete file of the given format and returns
    /// its bytes (parquet binary, gzip-compressed CSV, or a JSON array of
    /// records).
    fn generate(
        &self,
        rows: &[OpenDataMeasurement],
        format: Format,
    ) -> Result<Vec<u8>, DomainError>;
}
