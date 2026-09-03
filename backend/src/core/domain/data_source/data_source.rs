//! The persisted representation of an external data source.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::core::domain::assets::asset::value_objects::AssetId;

/// Namespace used to derive a deterministic data source id from its name.
const DATA_SOURCE_NAMESPACE: Uuid = Uuid::from_u128(0x9d3b_4f6e_2a1c_4d8e_9f0a_1b2c_3d4e_5f60);

#[derive(Debug, Clone)]
pub struct DataSource {
    pub id: value_objects::Id,
    pub name: value_objects::Name,
    pub provider_type: value_objects::ProviderType,
    /// The incremental import watermark: everything on/before this timestamp has
    /// been imported. `None` means "not yet imported" (full re-import).
    pub imported_until: Option<DateTime<Utc>>,
    /// Wall-clock time the data source was last successfully updated. Drives the
    /// "last updated" timestamps in the UI; unlike the coarse update job (which
    /// is `FAILED` when any single source fails) it survives partial successes.
    pub last_updated_at: Option<DateTime<Utc>>,
    /// Optional link to the asset holding the data source's logo. The data
    /// source **owns** the link (the asset repository is data-source-agnostic);
    /// it is set when the provider serves a logo image (otherwise the frontend
    /// falls back to the bundled data-source SVG).
    pub logo_asset_id: Option<AssetId>,
    /// Persisted provider logo hash used for hash-based change detection during
    /// import. `None` when the provider reports no logo.
    pub logo_sha256: Option<String>,
    /// The earliest measurement timestamp ever stored across the source's
    /// channels. Persisted and maintained by the import flow so the data-source
    /// detail page ("first data from", historical badge) never has to scan the
    /// measurement history. `None` while the source has no measurements.
    pub first_measurement_at: Option<DateTime<Utc>>,
    /// The latest measurement timestamp ever stored across the source's
    /// channels. Persisted and maintained by the import flow so the data-source
    /// detail page (recency, real-time badge) never has to scan the measurement
    /// history. `None` while the source has no measurements.
    pub last_measurement_at: Option<DateTime<Utc>>,
}

impl DataSource {
    pub fn new(name: String, provider_type: String) -> Self {
        Self {
            id: value_objects::Id(Self::id_from_name(&name)),
            name: value_objects::Name(name),
            provider_type: value_objects::ProviderType(provider_type),
            imported_until: None,
            last_updated_at: None,
            logo_asset_id: None,
            logo_sha256: None,
            first_measurement_at: None,
            last_measurement_at: None,
        }
    }

    /// Deterministic UUID (v5) derived from the data source name.
    pub fn id_from_name(name: &str) -> Uuid {
        Uuid::new_v5(&DATA_SOURCE_NAMESPACE, name.as_bytes())
    }
}

pub mod value_objects {
    use uuid::Uuid;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Id(pub Uuid);

    #[derive(Debug, Clone)]
    pub struct Name(pub String);

    #[derive(Debug, Clone)]
    pub struct ProviderType(pub String);
}

#[cfg(test)]
mod tests {
    use super::DataSource;

    #[test]
    fn id_is_deterministic_for_the_same_name() {
        assert_eq!(
            DataSource::id_from_name("Münster"),
            DataSource::id_from_name("Münster")
        );
    }

    #[test]
    fn id_differs_for_different_names() {
        assert_ne!(
            DataSource::id_from_name("Münster"),
            DataSource::id_from_name("Other")
        );
    }

    #[test]
    fn new_sets_provider_type() {
        let data_source = DataSource::new("Münster".to_string(), "github_zip".to_string());
        assert_eq!(data_source.name.0, "Münster");
        assert_eq!(data_source.provider_type.0, "github_zip");
    }

    #[test]
    fn new_starts_without_logo_or_watermark() {
        let data_source = DataSource::new("Münster".to_string(), "github_zip".to_string());
        assert_eq!(data_source.imported_until, None);
        assert_eq!(data_source.last_updated_at, None);
        assert_eq!(data_source.logo_asset_id, None);
        assert_eq!(data_source.logo_sha256, None);
        assert_eq!(data_source.first_measurement_at, None);
        assert_eq!(data_source.last_measurement_at, None);
    }
}
