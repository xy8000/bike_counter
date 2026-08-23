//! Business domain module for channels.
//!
//! - Model: [`channel::Channel`] (+ `value_objects`).
//! - Driven port: [`repository_port::ChannelRepository`] (implemented by
//!   `PostgresChannelRepository`).
//! - Driving port: [`service_port::ChannelServicePort`] (implemented by
//!   `ChannelService`).

pub mod channel;
pub mod repository_port;
pub mod service_port;
