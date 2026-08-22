//! The persisted representation of an external data source.

use uuid::Uuid;

/// Namespace used to derive a deterministic data source id from its name.
const DATA_SOURCE_NAMESPACE: Uuid = Uuid::from_u128(0x9d3b_4f6e_2a1c_4d8e_9f0a_1b2c_3d4e_5f60);

#[derive(Debug, Clone)]
pub struct DataSource {
    pub id: value_objects::Id,
    pub name: value_objects::Name,
    pub provider_type: value_objects::ProviderType,
}

impl DataSource {
    pub fn new(name: String, provider_type: String) -> Self {
        Self {
            id: value_objects::Id(Self::id_from_name(&name)),
            name: value_objects::Name(name),
            provider_type: value_objects::ProviderType(provider_type),
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
}
