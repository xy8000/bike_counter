//! Data-transfer objects for the REST API, split per resource.
//!
//! [`LinkDto`] and [`ErrorResponseDto`] are shared and live here; each resource
//! has its own submodule. Every DTO is re-exported at this module's root so
//! existing imports like `rest::dto::*` keep working unchanged.

mod channels;
mod counting_stations;
mod data_sources;
mod health;
mod jobs;
mod measurements;
mod persistent_state;
mod provider_message;
mod root;

pub use self::channels::*;
pub use self::counting_stations::*;
pub use self::data_sources::*;
pub use self::health::*;
pub use self::jobs::*;
pub use self::measurements::*;
pub use self::persistent_state::*;
pub use self::provider_message::*;
pub use self::root::*;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct LinkDto {
    pub href: String,
    /// `true` when `href` is an RFC 6570 URI template (e.g. contains a `{key}`
    /// placeholder) rather than a concrete URL.
    #[serde(default, skip_serializing_if = "is_false")]
    pub templated: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl LinkDto {
    pub fn new(href: impl Into<String>) -> Self {
        Self {
            href: href.into(),
            templated: false,
        }
    }

    /// Builds an RFC 6570 URI-template link (HAL `templated: true`).
    pub fn templated(href: impl Into<String>) -> Self {
        Self {
            href: href.into(),
            templated: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ErrorResponseDto {
    pub error: String,
}
