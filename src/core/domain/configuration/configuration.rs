pub struct Configuration {
    github_data_url: value_objects::raw_github_data_url,
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
    pub struct raw_github_data_url(String);
}