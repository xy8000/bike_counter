//! Driven (outbound) port for provisioning the self-hosted vector basemap
//! (`tiles/map.pmtiles`).
//!
//! Implemented by the `TilesInit` driven adapter (which drives the official
//! `go-pmtiles` CLI) and consumed by the [`TilesUpdateService`] for the
//! cron-scheduled refresh. [`main.rs`](crate) also calls [`ensure_available`]
//! during the startup init phase, so the server only becomes reachable once the
//! basemap exists.
//!
//! [`TilesUpdateService`]: crate::core::application::tiles_update_service::TilesUpdateService
//! [`ensure_available`]: TilesProvisioningPort::ensure_available

pub trait TilesProvisioningPort: Send + Sync {
    /// Ensures the basemap archive exists, building it if missing. Blocks until
    /// done; the application cannot run without the basemap.
    fn ensure_available(&self) -> Result<(), String>;

    /// Rebuilds the basemap and swaps it in atomically (a freshly built archive
    /// replaces the old one via rename), so the running application stays
    /// online during the (potentially long) extraction.
    fn update(&self) -> Result<(), String>;
}
