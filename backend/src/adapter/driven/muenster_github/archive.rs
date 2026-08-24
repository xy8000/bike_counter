//! In-memory archive index and zip extraction helpers.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use crate::core::domain::data_source::provider_port::{ChannelRecord, CountingStationRecord};

/// Archive internals (verified against the example archive).
pub const ARCHIVE_ROOT: &str = "radverkehr-zaehlstellen-main";
pub const SITE_INDEX_FILE: &str = "site_min.json";

/// In-memory representation of an extracted archive.
pub struct ArchiveIndex {
    pub stations: Vec<CountingStationRecord>,
    pub channels: Vec<ChannelRecord>,
    /// channel external id -> monthly CSV paths containing that channel.
    pub channel_csvs: HashMap<String, Vec<PathBuf>>,
    pub extracted_dir: PathBuf,
}

/// Turns a zip entry name into a safe relative path, rejecting any that would
/// escape the extraction directory.
pub fn sanitize_zip_path(name: &str) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for component in Path::new(name).components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            _ => return None,
        }
    }
    if out.as_os_str().is_empty() {
        None
    } else {
        Some(out)
    }
}
