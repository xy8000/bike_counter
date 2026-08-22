use crate::core::domain::configuration::configuration::value_objects::RawGithubDataUrl;

#[derive(Debug)]
pub struct Configuration {
    github_data_url: value_objects::RawGithubDataUrl,
    database: value_objects::DatabaseConfiguration,
}

impl Configuration {
    pub fn new(url: RawGithubDataUrl, database: value_objects::DatabaseConfiguration) -> Self {
        Self {
            github_data_url: url,
            database,
        }
    }

    pub fn github_data_url(&self) -> &RawGithubDataUrl {
        &self.github_data_url
    }

    pub fn database(&self) -> &value_objects::DatabaseConfiguration {
        &self.database
    }
}

pub mod value_objects {
    use crate::core::domain::configuration::error::ConfigError;

    #[derive(Debug, Clone)]
    pub struct RawGithubDataUrl(String);

    impl RawGithubDataUrl {
        pub fn new(url: String) -> Result<Self, ConfigError> {
            if url.trim().is_empty() {
                return Err(ConfigError::EmptyValue("github_data_url"));
            }

            Ok(Self(url))
        }

        pub fn as_str(&self) -> &str {
            &self.0
        }
    }

    #[derive(Debug, Clone)]
    pub struct DatabaseConfiguration {
        database_url: String,
        user: String,
        password: String,
        database_name: String,
    }

    impl DatabaseConfiguration {
        pub fn new(
            database_url: String,
            user: String,
            password: String,
            database_name: String,
        ) -> Result<Self, ConfigError> {
            let values = [
                ("database_url", database_url),
                ("database_user", user),
                ("database_password", password),
                ("database_name", database_name),
            ];
            if let Some((name, _)) = values.iter().find(|(_, value)| value.trim().is_empty()) {
                return Err(ConfigError::EmptyValue(name));
            }

            let [
                (_, database_url),
                (_, user),
                (_, password),
                (_, database_name),
            ] = values;
            Ok(Self {
                database_url,
                user,
                password,
                database_name,
            })
        }

        pub fn database_url(&self) -> &str {
            &self.database_url
        }

        pub fn user(&self) -> &str {
            &self.user
        }

        pub fn password(&self) -> &str {
            &self.password
        }

        pub fn database_name(&self) -> &str {
            &self.database_name
        }
    }
}

#[cfg(test)]
mod tests {
    use super::value_objects::{DatabaseConfiguration, RawGithubDataUrl};
    use crate::core::domain::configuration::error::ConfigError;

    #[test]
    fn rejects_empty_github_data_url() {
        assert!(matches!(
            RawGithubDataUrl::new("  ".to_string()),
            Err(ConfigError::EmptyValue("github_data_url"))
        ));
    }

    #[test]
    fn rejects_empty_database_values() {
        let cases = [
            ("database_url", "", "user", "password", "database"),
            ("database_user", "url", "", "password", "database"),
            ("database_password", "url", "user", "", "database"),
            ("database_name", "url", "user", "password", ""),
        ];

        for (name, database_url, user, password, database_name) in cases {
            assert!(matches!(
                DatabaseConfiguration::new(
                    database_url.to_string(),
                    user.to_string(),
                    password.to_string(),
                    database_name.to_string(),
                ),
                Err(ConfigError::EmptyValue(actual)) if actual == name
            ));
        }
    }
}
