//! Data-transfer objects for the BFF API.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Greeting returned by the BFF `hello` endpoint; the React frontend renders
/// its `message` on the page as proof that frontend -> BFF -> backend works.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct BffHelloDto {
    pub message: String,
}
