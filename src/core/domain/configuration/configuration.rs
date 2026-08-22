use crate::core::domain::configuration::configuration::value_objects::RawGithubDataUrl;

#[derive(Debug)]
pub struct Configuration {
    github_data_url: value_objects::RawGithubDataUrl,
}

impl Configuration {
    pub fn new(url: RawGithubDataUrl) -> Self {
        Self { github_data_url: url }
    }

    pub fn github_data_url(&self) -> &RawGithubDataUrl {
        &self.github_data_url
    }
}

pub mod value_objects {
    #[derive(Debug, Clone)]
    pub struct RawGithubDataUrl(String);

    impl RawGithubDataUrl {
        pub fn new(url: String) -> Self {
            Self(url)
        }

        pub fn as_str(&self) -> &str {
            &self.0
        }
    }
}