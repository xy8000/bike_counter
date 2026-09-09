//! Driven (outbound) port for provisioning the self-hosted vector basemap
//! (`tiles/map.pmtiles`).
//!
//! Implemented by the `TilesInit` driven adapter (which drives the official
//! `go-pmtiles` CLI) and consumed by the [`TilesUpdateService`]. Startup no
//! longer blocks on the basemap: the scheduled service builds it in the
//! background when [`TilesProvisioningPort::is_available`] reports the archive
//! missing, and the standalone `bike_counter tiles` subcommand (in
//! [`main.rs`](crate)) builds it out-of-band via [`ensure_available`].
//!
//! [`TilesUpdateService`]: crate::core::application::tiles_update_service::TilesUpdateService
//! [`ensure_available`]: TilesProvisioningPort::ensure_available

pub trait TilesProvisioningPort: Send + Sync {
    /// Whether the basemap archive already exists (no build is needed). A cheap
    /// file-existence check; the scheduler uses it to trigger the background
    /// build when the archive is missing (first boot or a wiped tiles dir).
    fn is_available(&self) -> bool;

    /// Ensures the basemap archive exists, building it if missing. Blocks until
    /// done; used by the standalone `bike_counter tiles` subcommand.
    fn ensure_available(&self) -> Result<(), String>;

    /// Rebuilds the basemap and swaps it in atomically (a freshly built archive
    /// replaces the old one via rename), so the running application stays
    /// online during the (potentially long) extraction.
    fn update(&self) -> Result<(), String>;
}
