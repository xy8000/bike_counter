//! Business domain module for external data sources.
//!
//! - Models: [`data_source::DataSource`] (+ `value_objects`),
//!   [`provider_message::ProviderMessage`] + [`provider_message::ProviderMessageSeverity`].
//! - Driven ports (implemented by the driven adapter):
//!   [`repository_port::DataSourceRepository`],
//!   [`persistent_state_port::PersistentStateStore`] + [`persistent_state_port::PersistentStateHandleFactory`],
//!   [`provider_message_port::ProviderMessageStore`],
//!   [`provider_port::DataProvider`] family (+ [`provider_port::ProviderMessageSinkFactory`]),
//!   [`data_provider_factory_port::DataProviderFactory`].
//! - Driving ports (implemented by the application services):
//!   [`service_port`] (`DataSourceServicePort`, `ProviderMessageServicePort`,
//!   `PersistentStateServicePort`, `DataSourceUpdateServicePort`).

#![allow(clippy::module_inception)]
pub mod data_provider_factory_port;
pub mod data_source;
pub mod persistent_state_port;
pub mod provider_message;
pub mod provider_message_port;
pub mod provider_port;
pub mod repository_port;
pub mod service_port;
