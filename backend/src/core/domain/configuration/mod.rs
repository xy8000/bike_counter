//! Business domain module for application configuration.
//!
//! - Model: [`configuration::Configuration`] (+ `value_objects`), [`error::ConfigError`].
//! - Driven port: [`repository_port::ConfigurationRepository`] (implemented by
//!   `ConfigurationTomlAdapter`).

#![allow(clippy::module_inception)]
pub mod configuration;
pub mod error;
pub mod repository_port;
