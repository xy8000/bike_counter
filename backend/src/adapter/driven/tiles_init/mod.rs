//! Driven adapter that provisions the self-hosted vector basemap
//! (`tiles/map.pmtiles`) by driving the official `go-pmtiles` CLI as a
//! subprocess.
//!
//! The basemap is **mandatory**: [`TilesInit::ensure_available`] is called
//! during the startup init phase (before the HTTP server binds) and builds the
//! archive if it is missing. The cron-scheduled
//! [`TilesUpdateService`](crate::core::application::tiles_update_service::TilesUpdateService)
//! calls [`TilesInit::update`], which builds into a temporary file and swaps it
//! in atomically (`fs::rename`) so nginx keeps serving a complete archive at
//! all times.
//!
//! The `go-pmtiles` CLI is downloaded once (pinned by
//! `maps.go_pmtiles_version`) into `<tiles_dir>/.pmtiles-bin` and cached across
//! runs. The Germany bounding box is hard-coded here for now.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use flate2::read::GzDecoder;
use tar::Archive as TarArchive;

use crate::core::domain::configuration::configuration::value_objects::MapsConfiguration;
use crate::core::domain::tiles::provisioning_port::TilesProvisioningPort;

/// Germany bounding box (`min_lon,min_lat,max_lon,max_lat`) — hard-coded for now.
pub const GERMANY_BBOX: &str = "5.8,47.2,15.1,55.1";
/// Basemap archive name written into the tiles directory.
pub const MAP_ARCHIVE: &str = "map.pmtiles";
/// Temporary archive name; renamed atomically over `MAP_ARCHIVE` on success.
const TMP_ARCHIVE: &str = "map.pmtiles.tmp";
/// Intermediate worldwide backdrop extract.
const WORLD_ARCHIVE: &str = "world.pmtiles";
/// Intermediate Germany detail extract.
const GERMANY_ARCHIVE: &str = "germany.pmtiles";
/// How many times a transient `extract` failure is retried.
const EXTRACT_ATTEMPTS: u32 = 3;
/// Release asset architecture suffix (the backend image is linux/amd64).
const RELEASE_ARCH: &str = "Linux_x86_64";

pub struct TilesInit {
    maps: MapsConfiguration,
    /// Output directory for the archive (env `TILES_DIR`, default `/data`).
    tiles_dir: PathBuf,
    /// Directory caching the downloaded `pmtiles` CLI binary.
    bin_dir: PathBuf,
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
        }
    }

    fn archive(&self) -> PathBuf {
        self.tiles_dir.join(MAP_ARCHIVE)
    }

    /// Ensures the basemap exists, building it if missing. Blocks until done.
    pub fn ensure_available(&self) -> Result<(), String> {
        if self.archive().exists() {
            println!("tiles/{MAP_ARCHIVE} already exists — nothing to do");
            return Ok(());
        }
        println!("tiles/{MAP_ARCHIVE} is missing — building it (this can take minutes)");
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
        println!("tiles/{MAP_ARCHIVE} updated");
        Ok(())
    }

    /// Builds the archive at `target`: extract the worldwide backdrop (z0-5),
    /// extract the Germany detail (z6-15, hard-coded bbox), merge both, and
    /// remove the intermediate archives.
    fn build_to(&self, target: &Path) -> Result<(), String> {
        fs::create_dir_all(&self.tiles_dir)
            .map_err(|e| format!("failed to create {}: {e}", self.tiles_dir.display()))?;
        let cli = self.ensure_cli()?;

        let world = self.tiles_dir.join(WORLD_ARCHIVE);
        let germany = self.tiles_dir.join(GERMANY_ARCHIVE);
        self.cleanup_intermediates();

        self.extract_with_retry(&cli, &world, &["--maxzoom=5"])?;
        let bbox_flag = format!("--bbox={GERMANY_BBOX}");
        self.extract_with_retry(
            &cli,
            &germany,
            &[bbox_flag.as_str(), "--minzoom=6", "--maxzoom=15"],
        )?;

        println!("Merging into tiles/{MAP_ARCHIVE} ...");
        self.run(
            &cli,
            &[
                "merge",
                world.to_str().ok_or("world path is not UTF-8")?,
                germany.to_str().ok_or("germany path is not UTF-8")?,
                target.to_str().ok_or("target path is not UTF-8")?,
            ],
        )?;

        self.cleanup_intermediates();
        println!("tiles/{MAP_ARCHIVE} ready at {}", target.display());
        Ok(())
    }

    /// Removes leftover intermediate extracts (safe no-op if absent).
    fn cleanup_intermediates(&self) {
        let _ = fs::remove_file(self.tiles_dir.join(WORLD_ARCHIVE));
        let _ = fs::remove_file(self.tiles_dir.join(GERMANY_ARCHIVE));
    }

    /// Runs `pmtiles extract` against the configured Protomaps source with
    /// retry, streaming the CLI's own progress to stdout.
    fn extract_with_retry(&self, cli: &Path, dest: &Path, flags: &[&str]) -> Result<(), String> {
        let source = self.maps.protomaps_build_url();
        let dest_str = dest.to_str().ok_or("destination path is not UTF-8")?;
        let mut args: Vec<&str> = vec!["extract", source, dest_str];
        args.extend_from_slice(flags);

        let mut attempt = 1u32;
        loop {
            println!(
                "Extracting {} (attempt {attempt}/{EXTRACT_ATTEMPTS}) ...",
                dest.file_name()
                    .map(|name| name.to_string_lossy())
                    .unwrap_or_default()
            );
            match self.run(cli, &args) {
                Ok(()) => return Ok(()),
                Err(error) if attempt < EXTRACT_ATTEMPTS => {
                    println!(
                        "extract failed (attempt {attempt}/{EXTRACT_ATTEMPTS}): {error}; retrying ..."
                    );
                    attempt += 1;
                }
                Err(error) => return Err(error),
            }
        }
    }

    /// Runs `command` with `args`, inheriting stdout/stderr (so the CLI's
    /// multi-minute progress is visible in the container logs) and returning an
    /// error when the exit status is non-zero.
    fn run(&self, command: &Path, args: &[&str]) -> Result<(), String> {
        println!("> {} {}", command.display(), args.join(" "));
        let status = Command::new(command)
            .args(args)
            .status()
            .map_err(|e| format!("failed to spawn {}: {e}", command.display()))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("{} exited with {status}", command.display()))
        }
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

        println!("Downloading pmtiles CLI v{version} ...");
        let response = ureq::get(&url)
            .call()
            .map_err(|e| format!("failed to download {url}: {e}"))?;
        let mut reader = response.into_body().into_reader();
        let mut file = fs::File::create(&tarball)
            .map_err(|e| format!("failed to create {}: {e}", tarball.display()))?;
        std::io::copy(&mut reader, &mut file)
            .map_err(|e| format!("failed to write {}: {e}", tarball.display()))?;
        drop(file);

        println!("Extracting pmtiles CLI ...");
        let tar_gz = fs::File::open(&tarball)
            .map_err(|e| format!("failed to open {}: {e}", tarball.display()))?;
        let decoder = GzDecoder::new(tar_gz);
        let mut archive = TarArchive::new(decoder);
        archive
            .unpack(&self.bin_dir)
            .map_err(|e| format!("failed to extract {}: {e}", tarball.display()))?;
        let _ = fs::remove_file(&tarball);

        make_executable(&bin);
        println!("pmtiles CLI ready at {}", bin.display());
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

impl TilesProvisioningPort for TilesInit {
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
            "https://build.protomaps.com/20260829.pmtiles".to_string(),
            "1.31.2".to_string(),
        )
        .unwrap()
    }

    /// Writes an executable fake `pmtiles` CLI that creates its destination
    /// archive file (extract -> argv[2], merge -> argv[3]) and exits 0.
    #[cfg(unix)]
    fn write_fake_cli(bin_dir: &Path) {
        fs::create_dir_all(bin_dir).unwrap();
        let script = "#!/bin/sh\n\
                      cmd=\"$1\"\n\
                      shift\n\
                      if [ \"$cmd\" = \"merge\" ]; then\n\
                        dest=\"$3\"\n\
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

    #[test]
    #[cfg(unix)]
    fn ensure_available_builds_the_archive_when_missing() {
        let root = temp_dir("build_missing");
        let tiles_dir = root.join("tiles");
        let bin_dir = root.join("bin");
        write_fake_cli(&bin_dir);

        let init = TilesInit::with_dirs(maps_configuration(), tiles_dir.clone(), bin_dir);

        init.ensure_available().unwrap();

        assert!(tiles_dir.join(MAP_ARCHIVE).exists());
        // Intermediates are cleaned up.
        assert!(!tiles_dir.join(WORLD_ARCHIVE).exists());
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
    #[cfg(unix)]
    fn update_builds_to_temp_and_swaps_atomically() {
        let root = temp_dir("update");
        let tiles_dir = root.join("tiles");
        let bin_dir = root.join("bin");
        write_fake_cli(&bin_dir);
        fs::create_dir_all(&tiles_dir).unwrap();
        // Existing archive that must survive until the swap.
        fs::write(tiles_dir.join(MAP_ARCHIVE), b"old").unwrap();

        let init = TilesInit::with_dirs(maps_configuration(), tiles_dir.clone(), bin_dir);

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
        let root = temp_dir("update_failure");
        let tiles_dir = root.join("tiles");
        let bin_dir = root.join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        // A non-executable `pmtiles` entry short-circuits the CLI download and
        // makes every subprocess fail fast (no network in unit tests).
        fs::write(bin_dir.join("pmtiles"), b"not executable").unwrap();
        let init = TilesInit::with_dirs(maps_configuration(), tiles_dir.clone(), bin_dir);

        let result = init.update();

        assert!(result.is_err());
        assert!(!tiles_dir.join(MAP_ARCHIVE).exists());
        assert!(!tiles_dir.join(TMP_ARCHIVE).exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn germany_bbox_is_hard_coded() {
        assert_eq!(GERMANY_BBOX, "5.8,47.2,15.1,55.1");
    }
}
