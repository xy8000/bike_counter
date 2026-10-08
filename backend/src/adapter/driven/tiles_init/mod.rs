//! Driven adapter that provisions the self-hosted vector basemap
//! (`tiles/map.pmtiles`) by driving the official `go-pmtiles` CLI as a
//! subprocess.
//!
//! The basemap is **mandatory** but is built in the background: startup no
//! longer blocks on it (the HTTP server binds immediately), and the
//! cron-scheduled
//! [`TilesUpdateService`](crate::core::application::tiles_update_service::TilesUpdateService)
//! drives the build on first boot (or whenever the archive is missing) and
//! refreshes it, building into a temporary file and swapping it in atomically
//! (`fs::rename`) so nginx keeps serving a complete archive at all times.
//! [`TilesInit::ensure_available`] remains for the standalone
//! `bike_counter tiles` subcommand (`make tiles`).
//!
//! The `go-pmtiles` CLI is downloaded once (pinned by
//! `maps.go_pmtiles_version`) into `<tiles_dir>/.pmtiles-bin` and cached across
//! runs. The Germany and surroundings bounding boxes are hard-coded here for now.
//!
//! The Protomaps **source** is resolved per build ([`resolve_source_default`]):
//! the configured `maps.protomaps_build_url` pin is used while it still
//! resolves, and otherwise the newest build in Protomaps' public catalog is
//! used — so a pin that has been pruned upstream self-heals instead of failing
//! the job.

use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use flate2::read::GzDecoder;
use tar::Archive as TarArchive;

use crate::adapter::driven::http::{DEFAULT_REQUEST_TIMEOUT_SECS, timed_agent};
use crate::core::domain::configuration::configuration::value_objects::MapsConfiguration;
use crate::core::domain::tiles::provisioning_port::TilesProvisioningPort;

/// Germany bounding box (`min_lon,min_lat,max_lon,max_lat`) — hard-coded for now.
pub const GERMANY_BBOX: &str = "5.8,47.2,15.1,55.1";
/// Surroundings bounding box (`min_lon,min_lat,max_lon,max_lat`) — a wide
/// Western/Central Europe box around Germany, hard-coded for now. It is
/// rendered at z6-z7 (two zoom layers beyond the world backdrop's z0-5) so the
/// Germany bbox edge no longer shows as a seam at mid zoom.
pub const SURROUNDINGS_BBOX: &str = "-11.112889,43.555498,27.187828,57.470545";
/// Basemap archive name written into the tiles directory.
pub const MAP_ARCHIVE: &str = "map.pmtiles";
/// Temporary archive name; renamed atomically over `MAP_ARCHIVE` on success.
const TMP_ARCHIVE: &str = "map.pmtiles.tmp";
/// Intermediate worldwide backdrop extract.
const WORLD_ARCHIVE: &str = "world.pmtiles";
/// Intermediate surroundings (around Germany) detail extract.
const SURROUNDINGS_ARCHIVE: &str = "surroundings.pmtiles";
/// Intermediate Germany detail extract.
const GERMANY_ARCHIVE: &str = "germany.pmtiles";
/// How many times a transient `extract` failure is retried.
const EXTRACT_ATTEMPTS: u32 = 3;
/// Release asset architecture suffix (the backend image is linux/amd64).
const RELEASE_ARCH: &str = "Linux_x86_64";
/// Protomaps build **catalog** — a JSON array of the currently available daily
/// builds (`[{"key":"YYYYMMDD.pmtiles", ...}, ...]`). Protomaps keeps only a
/// short window (~a month) of dailies, so this is used to self-heal a pin that
/// has been pruned upstream.
const BUILDS_CATALOG_URL: &str = "https://build-metadata.protomaps.dev/builds.json";
/// Base URL a catalog `key` (bare filename) is appended to.
const BUILDS_BASE_URL: &str = "https://build.protomaps.com/";

pub struct TilesInit {
    maps: MapsConfiguration,
    /// Output directory for the archive (env `TILES_DIR`, default `/data`).
    tiles_dir: PathBuf,
    /// Directory caching the downloaded `pmtiles` CLI binary.
    bin_dir: PathBuf,
    /// Resolves the Protomaps build URL to extract from (see
    /// [`resolve_source_default`]); injectable so tests stay offline.
    source_resolver: fn(&str) -> Result<String, String>,
}

impl TilesInit {
    /// Creates the adapter with the default directory layout: output to
    /// `$TILES_DIR` (default `/data`) and CLI cache `<tiles_dir>/.pmtiles-bin`.
    pub fn new(maps: MapsConfiguration) -> Self {
        let tiles_dir = env::var("TILES_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/data"));
        let bin_dir = tiles_dir.join(".pmtiles-bin");
        Self::with_dirs(maps, tiles_dir, bin_dir)
    }

    /// Creates the adapter with explicit directories (used by tests).
    pub fn with_dirs(maps: MapsConfiguration, tiles_dir: PathBuf, bin_dir: PathBuf) -> Self {
        Self {
            maps,
            tiles_dir,
            bin_dir,
            source_resolver: resolve_source_default,
        }
    }

    fn archive(&self) -> PathBuf {
        self.tiles_dir.join(MAP_ARCHIVE)
    }

    /// Whether the basemap archive already exists (no build is needed).
    pub fn is_available(&self) -> bool {
        self.archive().exists()
    }

    /// Ensures the basemap exists, building it if missing. Blocks until done;
    /// used by the standalone `bike_counter tiles` subcommand (`make tiles`).
    pub fn ensure_available(&self) -> Result<(), String> {
        if self.is_available() {
            tracing::info!("tiles/{MAP_ARCHIVE} already exists — nothing to do");
            return Ok(());
        }
        tracing::info!("tiles/{MAP_ARCHIVE} is missing — building it (this can take minutes)");
        self.build_to(&self.archive())
    }

    /// Rebuilds the basemap and swaps it in atomically so the running app stays
    /// online (nginx always serves either the old or the new complete archive).
    pub fn update(&self) -> Result<(), String> {
        let tmp = self.tiles_dir.join(TMP_ARCHIVE);
        let result = self.build_to(&tmp);
        if let Err(error) = &result {
            // Never leave a partial archive behind.
            let _ = fs::remove_file(&tmp);
            self.cleanup_intermediates();
            return Err(error.clone());
        }
        fs::rename(&tmp, self.archive())
            .map_err(|e| format!("failed to swap tiles/{MAP_ARCHIVE} into place: {e}"))?;
        tracing::info!("tiles/{MAP_ARCHIVE} updated");
        Ok(())
    }

    /// Builds the archive at `target`: extract the worldwide backdrop (z0-5),
    /// extract the surroundings detail (z6-7, hard-coded bbox around Germany —
    /// two zoom levels beyond the world), extract the Germany detail (z8-15,
    /// hard-coded bbox), and merge the three **disjoint** zoom bands into
    /// `target`, then remove the intermediate archives.
    ///
    /// The bands are disjoint by zoom range (z0-5 / z6-7 / z8-15), which
    /// `pmtiles merge` requires — it refuses overlapping inputs. The
    /// surroundings bbox fully contains Germany, so at z6-7 Germany is covered
    /// by the surroundings extract with the identical source tiles.
    ///
    /// The Protomaps source is resolved via the injected `source_resolver`
    /// (default [`resolve_source_default`]), which self-heals a pruned pin.
    fn build_to(&self, target: &Path) -> Result<(), String> {
        fs::create_dir_all(&self.tiles_dir)
            .map_err(|e| format!("failed to create {}: {e}", self.tiles_dir.display()))?;
        let cli = self.ensure_cli()?;
        let source = (self.source_resolver)(self.maps.protomaps_build_url())?;

        let world = self.tiles_dir.join(WORLD_ARCHIVE);
        let surroundings = self.tiles_dir.join(SURROUNDINGS_ARCHIVE);
        let germany = self.tiles_dir.join(GERMANY_ARCHIVE);
        self.cleanup_intermediates();

        self.extract_with_retry(&cli, &world, &["--maxzoom=5"], &source)?;
        let surroundings_bbox_flag = format!("--bbox={SURROUNDINGS_BBOX}");
        self.extract_with_retry(
            &cli,
            &surroundings,
            &[
                surroundings_bbox_flag.as_str(),
                "--minzoom=6",
                "--maxzoom=7",
            ],
            &source,
        )?;
        let bbox_flag = format!("--bbox={GERMANY_BBOX}");
        self.extract_with_retry(
            &cli,
            &germany,
            &[bbox_flag.as_str(), "--minzoom=8", "--maxzoom=15"],
            &source,
        )?;

        tracing::info!("Merging into tiles/{MAP_ARCHIVE} ...");
        self.run(
            &cli,
            &[
                "merge",
                world.to_str().ok_or("world path is not UTF-8")?,
                surroundings
                    .to_str()
                    .ok_or("surroundings path is not UTF-8")?,
                germany.to_str().ok_or("germany path is not UTF-8")?,
                target.to_str().ok_or("target path is not UTF-8")?,
            ],
        )?;

        self.cleanup_intermediates();
        tracing::info!("tiles/{MAP_ARCHIVE} ready at {}", target.display());
        Ok(())
    }

    /// Removes leftover intermediate extracts (safe no-op if absent).
    fn cleanup_intermediates(&self) {
        let _ = fs::remove_file(self.tiles_dir.join(WORLD_ARCHIVE));
        let _ = fs::remove_file(self.tiles_dir.join(SURROUNDINGS_ARCHIVE));
        let _ = fs::remove_file(self.tiles_dir.join(GERMANY_ARCHIVE));
    }

    /// Runs `pmtiles extract` against the resolved Protomaps `source` with
    /// retry, streaming the CLI's own progress to stdout.
    fn extract_with_retry(
        &self,
        cli: &Path,
        dest: &Path,
        flags: &[&str],
        source: &str,
    ) -> Result<(), String> {
        let dest_str = dest.to_str().ok_or("destination path is not UTF-8")?;
        let mut args: Vec<&str> = vec!["extract", source, dest_str];
        args.extend_from_slice(flags);

        let mut attempt = 1u32;
        loop {
            tracing::info!(
                "Extracting {} (attempt {attempt}/{EXTRACT_ATTEMPTS}) ...",
                dest.file_name()
                    .map(|name| name.to_string_lossy())
                    .unwrap_or_default()
            );
            match self.run(cli, &args) {
                Ok(()) => return Ok(()),
                Err(error) if attempt < EXTRACT_ATTEMPTS => {
                    tracing::info!(
                        "extract failed (attempt {attempt}/{EXTRACT_ATTEMPTS}): {error}; retrying ..."
                    );
                    attempt += 1;
                }
                Err(error) => return Err(error),
            }
        }
    }

    /// Runs `command` with `args`, streaming its stdout (so the CLI's
    /// multi-minute progress is visible in the container logs), capturing
    /// stderr and returning an error when the exit status is non-zero. The
    /// captured stderr is appended to the error — plus a hint when it looks like
    /// a pruned/`404` source — so a failed run is diagnosable from the job log
    /// instead of a bare exit status.
    fn run(&self, command: &Path, args: &[&str]) -> Result<(), String> {
        tracing::info!("> {} {}", command.display(), args.join(" "));
        let mut child = Command::new(command)
            .args(args)
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("failed to spawn {}: {e}", command.display()))?;
        let stderr = child
            .stderr
            .take()
            .map(|mut pipe| {
                let mut buffer = String::new();
                // The child's stdout is inherited (not a pipe), so draining
                // stderr to EOF here cannot deadlock on a full stdout buffer.
                let _ = pipe.read_to_string(&mut buffer);
                buffer
            })
            .unwrap_or_default();
        let status = child
            .wait()
            .map_err(|e| format!("failed to wait for {}: {e}", command.display()))?;
        if status.success() {
            return Ok(());
        }
        let detail = stderr.trim();
        let mut message = format!("{} exited with {status}", command.display());
        if !detail.is_empty() {
            message.push_str(": ");
            message.push_str(detail);
        }
        if detail.contains("404") {
            message.push_str(
                " (the pinned Protomaps build may have been pruned upstream; \
                 refresh maps.protomaps_build_url)",
            );
        }
        Err(message)
    }

    /// Ensures the pinned `pmtiles` CLI binary is cached, downloading and
    /// extracting it on first use.
    fn ensure_cli(&self) -> Result<PathBuf, String> {
        let version = self.maps.go_pmtiles_version();
        let bin = self.bin_dir.join("pmtiles");
        if bin.exists() {
            return Ok(bin);
        }

        fs::create_dir_all(&self.bin_dir)
            .map_err(|e| format!("failed to create {}: {e}", self.bin_dir.display()))?;

        let url = format!(
            "https://github.com/protomaps/go-pmtiles/releases/download/v{version}/go-pmtiles_{version}_{RELEASE_ARCH}.tar.gz"
        );
        let tarball = self.bin_dir.join("pmtiles.tar.gz");

        tracing::info!("Downloading pmtiles CLI v{version} ...");
        let response = ureq::get(&url)
            .call()
            .map_err(|e| format!("failed to download {url}: {e}"))?;
        let mut reader = response.into_body().into_reader();
        let mut file = fs::File::create(&tarball)
            .map_err(|e| format!("failed to create {}: {e}", tarball.display()))?;
        std::io::copy(&mut reader, &mut file)
            .map_err(|e| format!("failed to write {}: {e}", tarball.display()))?;
        drop(file);

        tracing::info!("Extracting pmtiles CLI ...");
        let tar_gz = fs::File::open(&tarball)
            .map_err(|e| format!("failed to open {}: {e}", tarball.display()))?;
        let decoder = GzDecoder::new(tar_gz);
        let mut archive = TarArchive::new(decoder);
        archive
            .unpack(&self.bin_dir)
            .map_err(|e| format!("failed to extract {}: {e}", tarball.display()))?;
        let _ = fs::remove_file(&tarball);

        make_executable(&bin);
        tracing::info!("pmtiles CLI ready at {}", bin.display());
        Ok(bin)
    }
}

/// Ensures the cached CLI binary is executable (release tarballs carry the exec
/// bit, but make it explicit to survive filesystem quirks).
#[cfg(unix)]
fn make_executable(bin: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(bin, fs::Permissions::from_mode(0o755));
}

#[cfg(not(unix))]
fn make_executable(_bin: &Path) {}

/// Resolves the Protomaps build to extract from: the **configured pin** while it
/// is still available, otherwise the **newest** build in the public catalog — so
/// a pin that has been pruned upstream (HTTP 404) self-heals instead of failing
/// the job.
fn resolve_source_default(configured: &str) -> Result<String, String> {
    resolve_source_from(configured, url_is_available, fetch_latest_build_key)
}

/// Source-resolution policy, with the two network effects injected so it can be
/// unit-tested without a network (see [`resolve_source_default`]).
fn resolve_source_from(
    configured: &str,
    is_available: impl Fn(&str) -> bool,
    latest_key: impl Fn() -> Result<String, String>,
) -> Result<String, String> {
    if is_available(configured) {
        return Ok(configured.to_string());
    }
    tracing::warn!(
        "configured Protomaps build {configured} is unavailable (pruned upstream?); \
         resolving the newest available build"
    );
    let key = latest_key()?;
    let url = format!("{BUILDS_BASE_URL}{key}");
    tracing::info!("using newest available Protomaps build {url}");
    Ok(url)
}

/// Whether `url` currently responds with a success status. The response body is
/// never read, so an available multi-gigabyte archive is not downloaded.
fn url_is_available(url: &str) -> bool {
    timed_agent(Duration::from_secs(DEFAULT_REQUEST_TIMEOUT_SECS))
        .get(url)
        .call()
        .is_ok()
}

/// Fetches the Protomaps build catalog and returns the newest `*.pmtiles` key.
fn fetch_latest_build_key() -> Result<String, String> {
    let body = timed_agent(Duration::from_secs(DEFAULT_REQUEST_TIMEOUT_SECS))
        .get(BUILDS_CATALOG_URL)
        .call()
        .map_err(|e| format!("failed to fetch the Protomaps build catalog: {e}"))?
        .into_body()
        .read_to_string()
        .map_err(|e| format!("failed to read the Protomaps build catalog: {e}"))?;
    latest_build_key(&body)
}

/// Picks the newest `*.pmtiles` key from the catalog JSON (an array of
/// `{"key": "...", ...}`), ordering by the `YYYYMMDD` filename.
fn latest_build_key(catalog_json: &str) -> Result<String, String> {
    #[derive(serde::Deserialize)]
    struct CatalogEntry {
        key: String,
    }
    let entries: Vec<CatalogEntry> = serde_json::from_str(catalog_json)
        .map_err(|e| format!("failed to parse the Protomaps build catalog: {e}"))?;
    entries
        .into_iter()
        .map(|entry| entry.key)
        .filter(|key| key.ends_with(".pmtiles"))
        .max()
        .ok_or_else(|| "the Protomaps build catalog listed no builds".to_string())
}

impl TilesProvisioningPort for TilesInit {
    fn is_available(&self) -> bool {
        self.is_available()
    }

    fn ensure_available(&self) -> Result<(), String> {
        self.ensure_available()
    }

    fn update(&self) -> Result<(), String> {
        self.update()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::core::domain::configuration::configuration::value_objects::MapsConfiguration;

    fn maps_configuration() -> MapsConfiguration {
        MapsConfiguration::new(
            "0 0 3 1 1,3,5,7,9,11 *".to_string(),
            7200,
            "https://build.protomaps.com/20261008.pmtiles".to_string(),
            "1.31.2".to_string(),
        )
        .unwrap()
    }

    /// Writes an executable fake `pmtiles` CLI that creates its destination
    /// archive file and exits 0. The merge destination is the **last** argument
    /// (extract keeps argv[2], which is the destination after `extract <source>
    /// <dest> ...`).
    #[cfg(unix)]
    fn write_fake_cli(bin_dir: &Path) {
        fs::create_dir_all(bin_dir).unwrap();
        let script = "#!/bin/sh\n\
                      cmd=\"$1\"\n\
                      shift\n\
                      if [ \"$cmd\" = \"merge\" ]; then\n\
                        while [ \"$#\" -gt 1 ]; do\n\
                          shift\n\
                        done\n\
                        dest=\"$1\"\n\
                      else\n\
                        dest=\"$2\"\n\
                      fi\n\
                      [ -n \"$dest\" ] && touch \"$dest\"\n\
                      exit 0\n";
        fs::write(bin_dir.join("pmtiles"), script).unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(bin_dir.join("pmtiles"), fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "bike_counter_tiles_{}_{}",
            std::process::id(),
            name
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Builds a `TilesInit` whose source resolution never touches the network (a
    /// stub that echoes the configured pin back).
    fn init_with_stub_source(tiles_dir: PathBuf, bin_dir: PathBuf) -> TilesInit {
        let mut init = TilesInit::with_dirs(maps_configuration(), tiles_dir, bin_dir);
        init.source_resolver = |configured| Ok(configured.to_string());
        init
    }

    /// Serialises the tests that spawn the fake `pmtiles` CLI: writing and
    /// exec'ing a fresh script from parallel test threads races with `fork` and
    /// intermittently fails with `Text file busy` (ETXTBSY).
    #[cfg(unix)]
    static CLI_SPAWN_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Acquires [`CLI_SPAWN_LOCK`], ignoring poisoning so one panicking test does
    /// not cascade into unrelated ETXTBSY failures.
    #[cfg(unix)]
    fn cli_spawn_guard() -> std::sync::MutexGuard<'static, ()> {
        CLI_SPAWN_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    #[cfg(unix)]
    fn ensure_available_builds_the_archive_when_missing() {
        let _guard = cli_spawn_guard();
        let root = temp_dir("build_missing");
        let tiles_dir = root.join("tiles");
        let bin_dir = root.join("bin");
        write_fake_cli(&bin_dir);

        let init = init_with_stub_source(tiles_dir.clone(), bin_dir);

        init.ensure_available().unwrap();

        assert!(tiles_dir.join(MAP_ARCHIVE).exists());
        // Intermediates are cleaned up.
        assert!(!tiles_dir.join(WORLD_ARCHIVE).exists());
        assert!(!tiles_dir.join(SURROUNDINGS_ARCHIVE).exists());
        assert!(!tiles_dir.join(GERMANY_ARCHIVE).exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ensure_available_is_a_noop_when_the_archive_exists() {
        let root = temp_dir("exists");
        let tiles_dir = root.join("tiles");
        fs::create_dir_all(&tiles_dir).unwrap();
        fs::write(tiles_dir.join(MAP_ARCHIVE), b"pmtiles").unwrap();
        // No CLI cache: a build would fail, so success proves the short-circuit.
        let init = TilesInit::with_dirs(maps_configuration(), tiles_dir.clone(), root.join("bin"));

        init.ensure_available().unwrap();
        assert!(tiles_dir.join(MAP_ARCHIVE).exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn is_available_reflects_archive_presence() {
        let root = temp_dir("is_available");
        let tiles_dir = root.join("tiles");
        fs::create_dir_all(&tiles_dir).unwrap();
        let init = TilesInit::with_dirs(maps_configuration(), tiles_dir.clone(), root.join("bin"));

        // Missing archive -> not available (the scheduler then builds it).
        assert!(!init.is_available());
        fs::write(tiles_dir.join(MAP_ARCHIVE), b"pmtiles").unwrap();
        assert!(init.is_available());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn update_builds_to_temp_and_swaps_atomically() {
        let _guard = cli_spawn_guard();
        let root = temp_dir("update");
        let tiles_dir = root.join("tiles");
        let bin_dir = root.join("bin");
        write_fake_cli(&bin_dir);
        fs::create_dir_all(&tiles_dir).unwrap();
        // Existing archive that must survive until the swap.
        fs::write(tiles_dir.join(MAP_ARCHIVE), b"old").unwrap();

        let init = init_with_stub_source(tiles_dir.clone(), bin_dir);

        init.update().unwrap();

        let archive = tiles_dir.join(MAP_ARCHIVE);
        assert!(archive.exists());
        // No temp archive is left behind.
        assert!(!tiles_dir.join(TMP_ARCHIVE).exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn update_reports_the_underlying_failure_without_leaving_temp_files() {
        let _guard = cli_spawn_guard();
        let root = temp_dir("update_failure");
        let tiles_dir = root.join("tiles");
        let bin_dir = root.join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        // A non-executable `pmtiles` entry short-circuits the CLI download and
        // makes every subprocess fail fast (no network in unit tests).
        fs::write(bin_dir.join("pmtiles"), b"not executable").unwrap();
        let init = init_with_stub_source(tiles_dir.clone(), bin_dir);

        let result = init.update();

        assert!(result.is_err());
        assert!(!tiles_dir.join(MAP_ARCHIVE).exists());
        assert!(!tiles_dir.join(TMP_ARCHIVE).exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn update_error_includes_cli_stderr() {
        let _guard = cli_spawn_guard();
        let root = temp_dir("stderr");
        let tiles_dir = root.join("tiles");
        let bin_dir = root.join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        // A fake CLI that fails (as `extract` does against a pruned/404 source)
        // and prints the reason to stderr.
        let script = "#!/bin/sh\n\
                      echo 'fetching: 404 Not Found' >&2\n\
                      exit 1\n";
        fs::write(bin_dir.join("pmtiles"), script).unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(bin_dir.join("pmtiles"), fs::Permissions::from_mode(0o755)).unwrap();
        let init = init_with_stub_source(tiles_dir.clone(), bin_dir);

        let error = init.update().unwrap_err();

        assert!(
            error.contains("404 Not Found"),
            "stderr missing from: {error}"
        );
        assert!(error.contains("pruned"), "hint missing from: {error}");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn latest_build_key_picks_the_newest_pmtiles_entry() {
        let catalog = r#"[
            {"key":"20230918.pmtiles","size":1},
            {"key":"20261007.pmtiles","size":2},
            {"key":"20261008.pmtiles","size":3},
            {"key":"notes.txt","size":0}
        ]"#;
        assert_eq!(latest_build_key(catalog).unwrap(), "20261008.pmtiles");
    }

    #[test]
    fn latest_build_key_errors_on_malformed_or_empty_catalogs() {
        assert!(latest_build_key("not json").is_err());
        assert!(latest_build_key("[]").is_err());
        assert!(latest_build_key(r#"[{"key":"notes.txt"}]"#).is_err());
    }

    #[test]
    fn resolve_source_prefers_the_configured_pin_when_available() {
        let configured = "https://build.protomaps.com/20261008.pmtiles";
        let fallback = || -> Result<String, String> { Ok("SHOULD-NOT-BE-USED".to_string()) };
        let resolved = resolve_source_from(configured, |_| true, fallback).unwrap();
        assert_eq!(resolved, configured);
    }

    #[test]
    fn resolve_source_falls_back_to_the_newest_catalog_build() {
        let latest = || -> Result<String, String> { Ok("20261008.pmtiles".to_string()) };
        let resolved = resolve_source_from(
            "https://build.protomaps.com/20260829.pmtiles",
            |_| false,
            latest,
        )
        .unwrap();
        assert_eq!(resolved, "https://build.protomaps.com/20261008.pmtiles");
    }

    #[test]
    fn resolve_source_propagates_a_catalog_failure() {
        let latest = || -> Result<String, String> { Err("catalog unavailable".to_string()) };
        let error = resolve_source_from(
            "https://build.protomaps.com/20260829.pmtiles",
            |_| false,
            latest,
        )
        .unwrap_err();
        assert_eq!(error, "catalog unavailable");
    }

    #[test]
    fn germany_bbox_is_hard_coded() {
        assert_eq!(GERMANY_BBOX, "5.8,47.2,15.1,55.1");
    }

    #[test]
    fn surroundings_bbox_is_hard_coded() {
        assert_eq!(
            SURROUNDINGS_BBOX,
            "-11.112889,43.555498,27.187828,57.470545"
        );
    }
}
