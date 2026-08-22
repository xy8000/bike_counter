use crate::{
    adapter::driven::configuration_toml_adapter::ConfigurationTomlAdapter,
    core::domain::configuration::repository::ConfigurationRepository,
};

mod adapter;
mod core;

fn main() {
    let configuration_repository: ConfigurationTomlAdapter =
        adapter::driven::configuration_toml_adapter::ConfigurationTomlAdapter::new(
            "config.toml".to_string(),
        );
    let configuration = configuration_repository
        .read_configuration()
        .expect("Failed to read configuration from file. Please check the file path and format.");
    println!("{:?}", configuration);
}
