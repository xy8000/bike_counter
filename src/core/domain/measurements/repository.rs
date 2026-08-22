use super::measurement::{Measurement, value_objects};
use crate::core::domain::error::DomainError;

pub trait MeasurementRepository {
    fn save(&self, measurement: Measurement) -> Result<(), DomainError>;
    fn save_batch(&self, measurements: Vec<Measurement>) -> Result<(), DomainError>;
    fn find_by_id(&self, id: value_objects::Id) -> Result<Measurement, DomainError>;
}
