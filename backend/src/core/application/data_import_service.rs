//! Imports counting stations, channels and measurements from the configured
//! external data sources. The runtime trigger (scheduling) is handled by the
//! `DataSourceUpdateService`; the capability itself is built and tested here.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::core::domain::assets::asset::Asset;
use crate::core::domain::assets::asset::value_objects::{ContentType, Sha256};
use crate::core::domain::assets::service_port::AssetServicePort;
use crate::core::domain::channels::channel::Channel;
use crate::core::domain::channels::channel::value_objects as channel_vo;
use crate::core::domain::channels::repository_port::ChannelRepository;
use crate::core::domain::configuration::configuration::value_objects::DataSourceConfiguration;
use crate::core::domain::counting_stations::counting_station::CountingStation;
use crate::core::domain::counting_stations::counting_station::value_objects as station_vo;
use crate::core::domain::counting_stations::repository_port::CountingStationRepository;
use crate::core::domain::data_source::data_source::value_objects::Id as DataSourceId;
use crate::core::domain::data_source::provider_port::{
    CountingStationRecord, DataProvider, MeasurementRecord,
};
use crate::core::domain::error::DomainError;
use crate::core::domain::measurements::measurement::Measurement;
use crate::core::domain::measurements::measurement::value_objects as measurement_vo;
use crate::core::domain::measurements::repository_port::MeasurementRepository;

/// A configured data source together with its built provider.
#[derive(Clone)]
pub struct DataSourceRuntime {
    pub configuration: DataSourceConfiguration,
    /// Deterministic id derived from the data source name.
    pub data_source_id: DataSourceId,
    pub provider: Arc<dyn DataProvider>,
}

/// Result of incrementally updating a single data source.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DataSourceUpdate {
    pub processed_measurements: usize,
    /// Number of measurements actually inserted (rows skipped by
    /// `ON CONFLICT DO NOTHING` are not counted).
    pub added_measurements: u64,
    /// Timestamp of the last processed measurement (the cursor to advance
    /// `data_sources.imported_until` to).
    pub last_measurement_timestamp: Option<DateTime<Utc>>,
    /// The earliest measurement timestamp processed this run. The caller merges
    /// it into the persisted `data_sources.first_measurement_at` (which only
    /// moves earlier), so a later historical backfill still shrinks the bound.
    pub first_measurement_timestamp: Option<DateTime<Utc>>,
    /// The latest measurement timestamp processed this run. The caller merges
    /// it into the persisted `data_sources.last_measurement_at` (which only
    /// moves later).
    pub last_measurement_timestamp_bound: Option<DateTime<Utc>>,
    /// `false` when a source-level run stopped early because the job deadline
    /// was reached (the watermark was checkpointed; the next run resumes).
    pub completed: bool,
}

/// The import phase a data source is currently in. The parallel job runner
/// persists it under the job metadata key `{data_source_id}_status` so a caller
/// can observe each source's progress while all sources import at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataSourceImportPhase {
    /// The run for the source just began.
    Starting,
    /// Counting stations are being synced from the provider.
    SyncingStations,
    /// Channels are being synced from the provider.
    SyncingChannels,
    /// Source-level measurement pages are being imported.
    ImportingMeasurements,
    /// The source import returned successfully (set by the caller).
    Finished,
    /// The source import failed (set by the caller).
    Failed,
}

impl DataSourceImportPhase {
    /// The canonical wire representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            DataSourceImportPhase::Starting => "STARTING",
            DataSourceImportPhase::SyncingStations => "SYNCING_STATIONS",
            DataSourceImportPhase::SyncingChannels => "SYNCING_CHANNELS",
            DataSourceImportPhase::ImportingMeasurements => "IMPORTING_MEASUREMENTS",
            DataSourceImportPhase::Finished => "FINISHED",
            DataSourceImportPhase::Failed => "FAILED",
        }
    }
}

/// A station is considered to still produce data when at least one of its
/// channels has a measurement within this many hours of `now`; otherwise it is
/// "not current" and is marked inactive after an import.
const STALE_STATION_AFTER_HOURS: i64 = 48;

pub struct DataImportService {
    counting_station_repository: Arc<dyn CountingStationRepository + Send + Sync>,
    channel_repository: Arc<dyn ChannelRepository + Send + Sync>,
    measurement_repository: Arc<dyn MeasurementRepository + Send + Sync>,
    runtimes: Vec<DataSourceRuntime>,
    /// Asset service used to sync station images during import. `None` (the
    /// default, used by unit tests and plain data imports) disables image
    /// handling entirely.
    asset_service: Option<Arc<dyn AssetServicePort>>,
}

impl DataImportService {
    pub fn new(
        counting_station_repository: Arc<dyn CountingStationRepository + Send + Sync>,
        channel_repository: Arc<dyn ChannelRepository + Send + Sync>,
        measurement_repository: Arc<dyn MeasurementRepository + Send + Sync>,
        runtimes: Vec<DataSourceRuntime>,
    ) -> Self {
        Self {
            counting_station_repository,
            channel_repository,
            measurement_repository,
            runtimes,
            asset_service: None,
        }
    }

    /// Enables hash-based station-image sync during import (used by the real
    /// backend wiring; tests keep the default `None`).
    pub fn with_asset_service(mut self, asset_service: Arc<dyn AssetServicePort>) -> Self {
        self.asset_service = Some(asset_service);
        self
    }

    /// Saves new counting stations and returns a map of `external_id -> station
    /// UUID` so channels can be linked to their station. The core owns all
    /// entity identity: every new station gets a fresh `Uuid::new_v4()`.
    fn sync_counting_stations(
        &self,
        runtime: &DataSourceRuntime,
    ) -> Result<HashMap<String, Uuid>, DomainError> {
        let stations = runtime
            .provider
            .get_all_counting_stations()
            .map_err(DomainError::from)?;

        // Resolve the built-in fallback image once per import (instead of once
        // per station): every station without a provider image points to it.
        let default_asset = match &self.asset_service {
            Some(service) => Some(service.default_asset()?),
            None => None,
        };

        let mut external_to_id = HashMap::with_capacity(stations.len());
        for record in stations {
            let external_id = station_vo::ExternalDatasourceId(record.external_id.clone());
            // Coordinates are optional: both lat/lng must be present.
            let coordinates = match (record.latitude, record.longitude) {
                (Some(latitude), Some(longitude)) => Some(station_vo::GeoCoordinates {
                    latitude,
                    longitude,
                }),
                _ => None,
            };

            let station = match self
                .counting_station_repository
                .find_by_external_datasource_id(external_id.clone())?
            {
                // The external id is stable: keep the identity and refresh the
                // mutable attributes (name, description, coordinates) from the
                // adapter on every sync.
                Some(existing) => {
                    let mut updated = CountingStation {
                        name: station_vo::Name(record.name.clone()),
                        description: station_vo::Description(record.description.clone()),
                        coordinates,
                        timezone: station_vo::Timezone(record.timezone.clone()),
                        // The provider still serves the station: reactivate it
                        // (it may have been marked inactive when it briefly
                        // disappeared from a previous provider output).
                        status: station_vo::Status::Active,
                        ..existing.clone()
                    };
                    if let (Some(asset_service), Some(default_asset)) =
                        (&self.asset_service, default_asset.as_ref())
                    {
                        self.sync_station_image(
                            asset_service,
                            &mut updated,
                            &record,
                            runtime.provider.as_ref(),
                            default_asset,
                        )?;
                    }
                    let changed = updated.name.0 != existing.name.0
                        || updated.description.0 != existing.description.0
                        || updated.coordinates != existing.coordinates
                        || updated.timezone != existing.timezone
                        || updated.image_asset_id != existing.image_asset_id
                        || updated.image_sha256 != existing.image_sha256
                        || updated.status != existing.status;
                    if changed {
                        self.counting_station_repository.update(updated.clone())?;
                    }
                    updated
                }
                None => {
                    let mut station = CountingStation {
                        id: station_vo::Id(Uuid::new_v4()),
                        name: station_vo::Name(record.name.clone()),
                        description: station_vo::Description(record.description.clone()),
                        external_datasource_id: Some(external_id),
                        data_source_id: Some(station_vo::DataSourceId(runtime.data_source_id.0)),
                        coordinates,
                        timezone: station_vo::Timezone(record.timezone.clone()),
                        image_asset_id: None,
                        image_sha256: None,
                        status: station_vo::Status::Active,
                    };
                    if let (Some(asset_service), Some(default_asset)) =
                        (&self.asset_service, default_asset.as_ref())
                    {
                        self.sync_station_image(
                            asset_service,
                            &mut station,
                            &record,
                            runtime.provider.as_ref(),
                            default_asset,
                        )?;
                    }
                    self.counting_station_repository.save(station.clone())?;
                    station
                }
            };
            external_to_id.insert(record.external_id, station.id.0);
        }

        // The provider's current station set is exactly the `external_to_id`
        // keys (new + refreshed above). Mark any station of this data source
        // that the provider no longer includes as inactive — it may still have
        // history, but it is not imported/refreshed anymore.
        let seen: HashSet<String> = external_to_id.keys().cloned().collect();
        for station in self.counting_station_repository.find_all()? {
            let is_this_source = station
                .data_source_id
                .is_some_and(|id| id == station_vo::DataSourceId(runtime.data_source_id.0));
            let missing = station
                .external_datasource_id
                .as_ref()
                .is_some_and(|external_id| !seen.contains(&external_id.0));
            if is_this_source && missing && station.status != station_vo::Status::Inactive {
                let mut updated = station;
                updated.status = station_vo::Status::Inactive;
                self.counting_station_repository.update(updated)?;
            }
        }

        Ok(external_to_id)
    }

    /// Hash-based station-image sync: point the station at the provider's image
    /// (downloading the bytes only when the reported hash changed) or at the
    /// built-in default when the provider reports no image.
    fn sync_station_image(
        &self,
        asset_service: &Arc<dyn AssetServicePort>,
        station: &mut CountingStation,
        record: &CountingStationRecord,
        provider: &dyn DataProvider,
        default_asset: &Asset,
    ) -> Result<(), DomainError> {
        let Some(provider_hash) = &record.image_sha256 else {
            // No provider image: fall back to the built-in default (idempotent).
            station.image_asset_id = Some(default_asset.id);
            station.image_sha256 = None;
            return Ok(());
        };

        // Hash unchanged since the last sync: keep the existing link, do not
        // download the bytes again.
        if station.image_sha256.as_deref() == Some(provider_hash.as_str()) {
            return Ok(());
        }

        // Hash changed (or the station has no link yet): ask the provider for
        // the new image bytes and persist them.
        match provider
            .get_station_image(&record.external_id)
            .map_err(DomainError::from)?
        {
            Some(image) => {
                let sha256 = Sha256::parse(&image.sha256)?;
                let content_type = ContentType::parse(&image.content_type)?;
                let asset =
                    asset_service.store_provider_image(sha256, content_type, &image.bytes)?;
                station.image_asset_id = Some(asset.id);
                station.image_sha256 = Some(provider_hash.clone());
            }
            None => {
                station.image_asset_id = Some(default_asset.id);
                station.image_sha256 = None;
            }
        }
        Ok(())
    }

    /// Saves new channels and returns every channel (new + existing) so the
    /// caller can resolve source-level measurements to persisted channel ids.
    /// Each new channel's `counting_station_id` is resolved from the station map
    /// built by [`Self::sync_counting_stations`].
    fn sync_channels(
        &self,
        runtime: &DataSourceRuntime,
        station_ids: &HashMap<String, Uuid>,
    ) -> Result<Vec<Channel>, DomainError> {
        let channels = runtime
            .provider
            .get_all_channels()
            .map_err(DomainError::from)?;

        let mut result = Vec::with_capacity(channels.len());
        for record in channels {
            let external_id = channel_vo::ExternalDatasourceId(record.external_id.clone());
            let existing = self
                .channel_repository
                .find_by_external_datasource_id(external_id.clone())?;

            match existing {
                Some(existing) => result.push(existing),
                None => {
                    let counting_station_id = *station_ids
                        .get(&record.counting_station_external_id)
                        .ok_or_else(|| {
                            DomainError::InvalidQuery(format!(
                                "channel '{}' references unknown station '{}'",
                                record.external_id, record.counting_station_external_id
                            ))
                        })?;
                    let channel = Channel {
                        id: channel_vo::Id(Uuid::new_v4()),
                        counting_station_id: channel_vo::CountingStationId(counting_station_id),
                        name: channel_vo::Name(record.name),
                        description: channel_vo::Description(record.description),
                        external_datasource_id: Some(external_id),
                    };
                    self.channel_repository.save(channel.clone())?;
                    result.push(channel);
                }
            }
        }

        Ok(result)
    }

    /// Incrementally updates a single data source in **strict order**:
    /// counting stations first, then channels, then measurements. Measurements
    /// are read through the provider's **source-level read**
    /// ([`DataProvider::get_measurements_source`]): the provider owns the
    /// per-channel interleaving and reports a safe watermark per batch, which is
    /// handed to `on_batch(processed, added, watermark)` so the caller can
    /// checkpoint `imported_until` — an interrupted run therefore resumes
    /// instead of reprocessing.
    ///
    /// `from` is the persisted `imported_until` (the anchor of this run; `None`
    /// requests a full read). `deadline` (the job's `lifetime_until`) makes the
    /// run stop early after the current batch with `completed: false`; the
    /// caller finishes the job gracefully and the next run resumes from the
    /// checkpoint.
    ///
    /// Returns how many measurements were processed and added and the timestamp
    /// of the last processed measurement (the cursor for the next run).
    pub fn update_data_source(
        &self,
        runtime: &DataSourceRuntime,
        from: Option<DateTime<Utc>>,
        deadline: Option<DateTime<Utc>>,
        on_batch: impl Fn(usize, u64, Option<DateTime<Utc>>) -> Result<(), DomainError>,
    ) -> Result<DataSourceUpdate, DomainError> {
        self.update_data_source_with_progress(runtime, from, deadline, on_batch, |_| Ok(()))
    }

    /// [`Self::update_data_source`] variant that additionally reports the source's
    /// import phase through `on_phase` (the parallel job runner uses it to persist
    /// the per-data-source `{data_source_id}_status` metadata). `update_data_source`
    /// delegates here with a no-op phase reporter, so existing callers and tests
    /// are unaffected.
    pub fn update_data_source_with_progress(
        &self,
        runtime: &DataSourceRuntime,
        from: Option<DateTime<Utc>>,
        deadline: Option<DateTime<Utc>>,
        on_batch: impl Fn(usize, u64, Option<DateTime<Utc>>) -> Result<(), DomainError>,
        mut on_phase: impl FnMut(DataSourceImportPhase) -> Result<(), DomainError>,
    ) -> Result<DataSourceUpdate, DomainError> {
        on_phase(DataSourceImportPhase::Starting)?;
        let station_ids = {
            on_phase(DataSourceImportPhase::SyncingStations)?;
            self.sync_counting_stations(runtime)?
        };
        let channels = {
            on_phase(DataSourceImportPhase::SyncingChannels)?;
            self.sync_channels(runtime, &station_ids)?
        };

        // Resolve each channel's external id (as the provider tags source-level
        // measurements) to its persisted channel id.
        let channel_by_external: HashMap<String, Uuid> = channels
            .iter()
            .filter_map(|channel| {
                channel
                    .external_datasource_id
                    .as_ref()
                    .map(|external| (external.0.clone(), channel.id.0))
            })
            .collect();

        let max_batch_size = runtime.provider.max_measurement_batch_size();
        let mut processed = 0usize;
        let mut added = 0u64;
        let mut watermark: Option<DateTime<Utc>> = None;
        // Source-wide earliest/latest measurement timestamp seen this run. The
        // caller persists them as the data source's measurement bounds; folding
        // over every processed record is safe because rows skipped by
        // `ON CONFLICT DO NOTHING` share timestamps already inside the bounds.
        let mut run_first: Option<DateTime<Utc>> = None;
        let mut run_last: Option<DateTime<Utc>> = None;
        let mut completed = true;

        // The provider's scanner is anchored at `from` for the whole run; the
        // reported `next_from` is only the checkpoint watermark, never the next
        // call's anchor.
        on_phase(DataSourceImportPhase::ImportingMeasurements)?;
        loop {
            let batch = runtime
                .provider
                .get_measurements_source(from, max_batch_size)
                .map_err(DomainError::from)?;

            processed += batch.measurements.len();

            // Group the (possibly multi-channel) page per persisted channel.
            let mut by_channel: HashMap<Uuid, Vec<MeasurementRecord>> = HashMap::new();
            for source in batch.measurements {
                // The timestamp is copied before `source.record` is moved into
                // the per-channel batch below.
                let timestamp = source.record.timestamp;
                run_first = Some(run_first.map_or(timestamp, |earliest| earliest.min(timestamp)));
                run_last = Some(run_last.map_or(timestamp, |latest| latest.max(timestamp)));
                let channel_id = channel_by_external
                    .get(&source.channel_external_id)
                    .ok_or_else(|| {
                        DomainError::InvalidQuery(format!(
                            "provider returned a measurement for unknown channel '{}'",
                            source.channel_external_id
                        ))
                    })?;
                by_channel
                    .entry(*channel_id)
                    .or_default()
                    .push(source.record);
            }
            for (channel_id, records) in by_channel {
                added += self
                    .measurement_repository
                    .save_batch(to_measurements(records, channel_id))?;
            }

            if batch.next_from.is_some() {
                watermark = batch.next_from;
            }
            on_batch(processed, added, batch.next_from)?;

            if !batch.more {
                break;
            }
            if deadline.is_some_and(|deadline| Utc::now() >= deadline) {
                completed = false;
                break;
            }
        }

        // Only after the whole source was imported: stations without current
        // data are decommissioned.
        if completed {
            self.mark_stale_stations_inactive(runtime, Utc::now())?;
        }

        Ok(DataSourceUpdate {
            processed_measurements: processed,
            added_measurements: added,
            last_measurement_timestamp: watermark,
            first_measurement_timestamp: run_first,
            last_measurement_timestamp_bound: run_last,
            completed,
        })
    }

    /// Marks a station of the data source inactive when it no longer has
    /// "current" data — none of its channels has a measurement newer than
    /// [`STALE_STATION_AFTER_HOURS`] before `now`. Only ever marks INACTIVE
    /// (never reactivates here): stations the provider still serves are set back
    /// to Active at the start of the next import by [`Self::sync_counting_stations`]
    /// and only stay active when the stale check finds current data again.
    fn mark_stale_stations_inactive(
        &self,
        runtime: &DataSourceRuntime,
        now: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        let cutoff = now - chrono::Duration::hours(STALE_STATION_AFTER_HOURS);
        let stations: Vec<CountingStation> = self
            .counting_station_repository
            .find_all()?
            .into_iter()
            .filter(|station| {
                station
                    .data_source_id
                    .is_some_and(|id| id == station_vo::DataSourceId(runtime.data_source_id.0))
            })
            .collect();

        // Gather every channel of the data source, keyed by its station id, so
        // the "latest measurement" check can be done per station in one query.
        let mut channels_by_station: HashMap<Uuid, Vec<measurement_vo::ChannelId>> = HashMap::new();
        let mut all_channel_ids: Vec<measurement_vo::ChannelId> = Vec::new();
        for station in &stations {
            let channels = self
                .channel_repository
                .find_by_counting_station_id(channel_vo::CountingStationId(station.id.0))?;
            let channel_ids: Vec<measurement_vo::ChannelId> = channels
                .iter()
                .map(|channel| measurement_vo::ChannelId(channel.id.0))
                .collect();
            all_channel_ids.extend(channel_ids.iter().copied());
            channels_by_station.insert(station.id.0, channel_ids);
        }

        let latest = self
            .measurement_repository
            .latest_by_channel(&all_channel_ids)?;
        let latest_by_channel: HashMap<Uuid, DateTime<Utc>> = latest
            .into_iter()
            .map(|entry| (entry.channel_id, entry.timestamp))
            .collect();

        for station in stations {
            let has_current_data =
                channels_by_station
                    .get(&station.id.0)
                    .is_some_and(|channel_ids| {
                        channel_ids.iter().any(|channel_id| {
                            latest_by_channel
                                .get(&channel_id.0)
                                .is_some_and(|timestamp| *timestamp >= cutoff)
                        })
                    });
            if !has_current_data && station.status != station_vo::Status::Inactive {
                let mut updated = station;
                updated.status = station_vo::Status::Inactive;
                self.counting_station_repository.update(updated)?;
            }
        }
        Ok(())
    }
}

/// Converts provider measurement records into persisted entities: the core
/// generates a fresh UUID per measurement and attaches the channel id.
fn to_measurements(records: Vec<MeasurementRecord>, channel_id: Uuid) -> Vec<Measurement> {
    records
        .into_iter()
        .map(|record| Measurement {
            id: measurement_vo::Id(Uuid::new_v4()),
            channel_id: measurement_vo::ChannelId(channel_id),
            value: measurement_vo::Value(record.value),
            timestamp: measurement_vo::Timestamp(record.timestamp),
            resolution_seconds: measurement_vo::ResolutionSeconds(record.resolution_seconds),
            interval_end: record.interval_end,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, VecDeque};
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::core::domain::assets::asset::value_objects::{
        AssetId, ByteSize, ContentType, ObjectKey, Sha256,
    };
    use crate::core::domain::assets::asset::{Asset, AssetOrigin, BuiltinImage};
    use crate::core::domain::assets::service_port::AssetServicePort;
    use crate::core::domain::channels::channel::value_objects as channel_vo;
    use crate::core::domain::configuration::configuration::value_objects::DataProviderConfiguration;
    use crate::core::domain::counting_stations::counting_station::CountingStation;
    use crate::core::domain::data_source::data_source::DataSource;
    use crate::core::domain::data_source::provider_port::{
        ChannelRecord, CountingStationRecord, MeasurementRecord, ProviderError, SourceMeasurement,
        SourceMeasurementBatch, StationImage,
    };
    use crate::core::domain::health::HealthStatus;
    use crate::core::domain::measurements::measurement::Measurement;
    use crate::core::domain::measurements::measurement::value_objects as measurement_vo;

    fn data_source_config(name: &str, provider_type: &str) -> DataSourceConfiguration {
        DataSourceConfiguration::new(
            name.to_string(),
            DataProviderConfiguration::new(provider_type.to_string(), HashMap::new()).unwrap(),
        )
        .unwrap()
    }

    fn station(external_id: &str) -> CountingStation {
        CountingStation {
            id: station_vo::Id(Uuid::new_v4()),
            name: station_vo::Name(format!("Station {external_id}")),
            description: station_vo::Description("desc".to_string()),
            external_datasource_id: Some(station_vo::ExternalDatasourceId(external_id.to_string())),
            data_source_id: None,
            coordinates: None,
            timezone: station_vo::Timezone("Europe/Berlin".to_string()),
            image_asset_id: None,
            image_sha256: None,
            status: station_vo::Status::Active,
        }
    }

    fn channel(external_id: &str) -> Channel {
        Channel {
            id: channel_vo::Id(Uuid::new_v4()),
            counting_station_id: channel_vo::CountingStationId(Uuid::new_v4()),
            name: channel_vo::Name(format!("Channel {external_id}")),
            description: channel_vo::Description("desc".to_string()),
            external_datasource_id: Some(channel_vo::ExternalDatasourceId(external_id.to_string())),
        }
    }

    fn station_record(external_id: &str) -> CountingStationRecord {
        CountingStationRecord {
            external_id: external_id.to_string(),
            name: format!("Station {external_id}"),
            description: "desc".to_string(),
            latitude: None,
            longitude: None,
            timezone: "Europe/Berlin".to_string(),
            image_sha256: None,
        }
    }

    fn channel_record(external_id: &str, station_external_id: &str) -> ChannelRecord {
        ChannelRecord {
            external_id: external_id.to_string(),
            counting_station_external_id: station_external_id.to_string(),
            name: format!("Channel {external_id}"),
            description: "desc".to_string(),
        }
    }

    fn measurement_record(value: i64, timestamp: DateTime<Utc>) -> MeasurementRecord {
        MeasurementRecord {
            value,
            timestamp,
            resolution_seconds: 3600,
            interval_end: None,
        }
    }

    fn source_measurement(
        channel_external_id: &str,
        record: MeasurementRecord,
    ) -> SourceMeasurement {
        SourceMeasurement {
            channel_external_id: channel_external_id.to_string(),
            record,
        }
    }

    fn source_batch(
        entries: Vec<(&str, MeasurementRecord)>,
        next_from: Option<DateTime<Utc>>,
        more: bool,
    ) -> SourceMeasurementBatch {
        SourceMeasurementBatch {
            measurements: entries
                .into_iter()
                .map(|(id, record)| source_measurement(id, record))
                .collect(),
            next_from,
            more,
        }
    }

    fn timestamp(rfc3339: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(rfc3339)
            .expect("valid timestamp")
            .with_timezone(&Utc)
    }

    /// A provider that serves source-level batches over a queue. Every call's
    /// `from` anchor is recorded so tests can verify the run resumes from the
    /// persisted `imported_until`.
    struct SourceMockProvider {
        stations: Vec<CountingStationRecord>,
        channels: Vec<ChannelRecord>,
        batches: Mutex<VecDeque<SourceMeasurementBatch>>,
        recorded_from: Mutex<Vec<Option<DateTime<Utc>>>>,
        batch_size: usize,
    }

    impl DataProvider for SourceMockProvider {
        fn check_health(&self) -> HealthStatus {
            HealthStatus::Up
        }

        fn get_all_counting_stations(&self) -> Result<Vec<CountingStationRecord>, ProviderError> {
            Ok(self.stations.clone())
        }

        fn get_all_channels(&self) -> Result<Vec<ChannelRecord>, ProviderError> {
            Ok(self.channels.clone())
        }

        fn get_measurements_source(
            &self,
            from: Option<DateTime<Utc>>,
            _max_batch_size: usize,
        ) -> Result<SourceMeasurementBatch, ProviderError> {
            self.recorded_from.lock().unwrap().push(from);
            self.batches
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| ProviderError::InvalidData("no more source batches".to_string()))
        }

        fn max_measurement_batch_size(&self) -> usize {
            self.batch_size
        }
    }

    /// A provider that serves stations with image hashes and scripted images
    /// (and no channels or measurements).
    struct ImageProvider {
        stations: Vec<CountingStationRecord>,
        images: Mutex<HashMap<String, StationImage>>,
        fetch_calls: Mutex<usize>,
    }

    impl DataProvider for ImageProvider {
        fn check_health(&self) -> HealthStatus {
            HealthStatus::Up
        }
        fn get_all_counting_stations(&self) -> Result<Vec<CountingStationRecord>, ProviderError> {
            Ok(self.stations.clone())
        }
        fn get_all_channels(&self) -> Result<Vec<ChannelRecord>, ProviderError> {
            Ok(Vec::new())
        }
        fn get_measurements_source(
            &self,
            _from: Option<DateTime<Utc>>,
            _max_batch_size: usize,
        ) -> Result<SourceMeasurementBatch, ProviderError> {
            Ok(SourceMeasurementBatch {
                measurements: Vec::new(),
                next_from: None,
                more: false,
            })
        }
        fn max_measurement_batch_size(&self) -> usize {
            500
        }
        fn get_station_image(
            &self,
            external_id: &str,
        ) -> Result<Option<StationImage>, ProviderError> {
            *self.fetch_calls.lock().unwrap() += 1;
            Ok(self.images.lock().unwrap().get(external_id).cloned())
        }
    }

    fn runtime(provider: Arc<dyn DataProvider>) -> DataSourceRuntime {
        DataSourceRuntime {
            configuration: data_source_config("Münster", "münster_opendata_github_provider"),
            data_source_id: DataSourceId(DataSource::id_from_name("Münster")),
            provider,
        }
    }

    /// An [`AssetServicePort`] mock that resolves a fixed default asset and
    /// records every provider-image store.
    struct MockAssetService {
        default: Asset,
        stored_provider: Mutex<usize>,
    }

    impl AssetServicePort for MockAssetService {
        fn sync_builtin_images(&self, _builtin: &[BuiltinImage]) -> Result<(), DomainError> {
            Ok(())
        }
        fn default_asset(&self) -> Result<Asset, DomainError> {
            Ok(self.default.clone())
        }
        fn store_provider_image(
            &self,
            sha256: Sha256,
            content_type: ContentType,
            bytes: &[u8],
        ) -> Result<Asset, DomainError> {
            *self.stored_provider.lock().unwrap() += 1;
            let now = Utc::now();
            Ok(Asset {
                id: AssetId(Uuid::new_v4()),
                object_key: ObjectKey(format!("provider/{}", sha256.0)),
                content_type,
                byte_size: ByteSize(bytes.len() as i64),
                sha256,
                origin: AssetOrigin::Provider,
                created_at: now,
                updated_at: now,
            })
        }
        fn find_by_id(&self, _id: AssetId) -> Result<Option<Asset>, DomainError> {
            Ok(None)
        }
    }

    fn default_asset() -> Asset {
        let now = Utc::now();
        Asset {
            id: AssetId(Uuid::from_u128(0xDEAD)),
            object_key: ObjectKey("builtin/bike-icon-black-transparent.svg".to_string()),
            content_type: ContentType("image/svg+xml".to_string()),
            byte_size: ByteSize(1),
            sha256: Sha256("a".repeat(64)),
            origin: AssetOrigin::Builtin,
            created_at: now,
            updated_at: now,
        }
    }

    fn image_service() -> Arc<MockAssetService> {
        Arc::new(MockAssetService {
            default: default_asset(),
            stored_provider: Mutex::new(0),
        })
    }

    struct MockCountingStationRepository {
        stations: Mutex<Vec<CountingStation>>,
    }

    impl CountingStationRepository for MockCountingStationRepository {
        fn save(&self, station: CountingStation) -> Result<(), DomainError> {
            self.stations.lock().unwrap().push(station);
            Ok(())
        }

        fn update(&self, station: CountingStation) -> Result<(), DomainError> {
            let mut stations = self.stations.lock().unwrap();
            if let Some(existing) = stations.iter_mut().find(|s| s.id == station.id) {
                *existing = station;
            }
            Ok(())
        }

        fn find_by_id(&self, id: station_vo::Id) -> Result<CountingStation, DomainError> {
            self.stations
                .lock()
                .unwrap()
                .iter()
                .find(|s| s.id == id)
                .cloned()
                .ok_or(DomainError::NotFound(id.0))
        }

        fn find_all(&self) -> Result<Vec<CountingStation>, DomainError> {
            Ok(self.stations.lock().unwrap().clone())
        }

        fn find_by_external_datasource_id(
            &self,
            external_id: station_vo::ExternalDatasourceId,
        ) -> Result<Option<CountingStation>, DomainError> {
            Ok(self
                .stations
                .lock()
                .unwrap()
                .iter()
                .find(|s| {
                    s.external_datasource_id.as_ref().map(|e| e.0.as_str())
                        == Some(external_id.0.as_str())
                })
                .cloned())
        }

        fn find_filtered(&self, name: Option<&str>) -> Result<Vec<CountingStation>, DomainError> {
            let stations = self.stations.lock().unwrap();
            Ok(match name {
                Some(name) => stations
                    .iter()
                    .filter(|s| s.name.0.to_lowercase().contains(&name.to_lowercase()))
                    .cloned()
                    .collect(),
                None => stations.clone(),
            })
        }
    }

    struct MockChannelRepository {
        channels: Mutex<Vec<Channel>>,
    }

    impl ChannelRepository for MockChannelRepository {
        fn save(&self, channel: Channel) -> Result<(), DomainError> {
            self.channels.lock().unwrap().push(channel);
            Ok(())
        }

        fn find_by_id(&self, id: channel_vo::Id) -> Result<Channel, DomainError> {
            self.channels
                .lock()
                .unwrap()
                .iter()
                .find(|c| c.id == id)
                .cloned()
                .ok_or(DomainError::NotFound(id.0))
        }

        fn find_all(&self) -> Result<Vec<Channel>, DomainError> {
            Ok(self.channels.lock().unwrap().clone())
        }

        fn find_by_counting_station_id(
            &self,
            station_id: channel_vo::CountingStationId,
        ) -> Result<Vec<Channel>, DomainError> {
            Ok(self
                .channels
                .lock()
                .unwrap()
                .iter()
                .filter(|c| c.counting_station_id == station_id)
                .cloned()
                .collect())
        }

        fn find_by_external_datasource_id(
            &self,
            external_id: channel_vo::ExternalDatasourceId,
        ) -> Result<Option<Channel>, DomainError> {
            Ok(self
                .channels
                .lock()
                .unwrap()
                .iter()
                .find(|c| {
                    c.external_datasource_id.as_ref().map(|e| e.0.as_str())
                        == Some(external_id.0.as_str())
                })
                .cloned())
        }

        fn find_filtered(
            &self,
            counting_station_id: Option<channel_vo::CountingStationId>,
            name: Option<&str>,
        ) -> Result<Vec<Channel>, DomainError> {
            let channels = self.channels.lock().unwrap();
            Ok(channels
                .iter()
                .filter(|c| {
                    counting_station_id.is_none_or(|id| c.counting_station_id == id)
                        && name.is_none_or(|n| c.name.0.to_lowercase().contains(&n.to_lowercase()))
                })
                .cloned()
                .collect())
        }
    }

    struct MockMeasurementRepository {
        measurements: Mutex<Vec<Measurement>>,
    }

    impl MeasurementRepository for MockMeasurementRepository {
        fn save(&self, measurement: Measurement) -> Result<(), DomainError> {
            self.measurements.lock().unwrap().push(measurement);
            Ok(())
        }

        fn save_batch(&self, measurements: Vec<Measurement>) -> Result<u64, DomainError> {
            let len = measurements.len() as u64;
            self.measurements.lock().unwrap().extend(measurements);
            Ok(len)
        }

        fn find_by_id(&self, id: measurement_vo::Id) -> Result<Measurement, DomainError> {
            self.measurements
                .lock()
                .unwrap()
                .iter()
                .find(|m| m.id.0 == id.0)
                .cloned()
                .ok_or(DomainError::NotFound(id.0))
        }

        fn find_all(&self) -> Result<Vec<Measurement>, DomainError> {
            Ok(self.measurements.lock().unwrap().clone())
        }

        fn find_by_channel_id(
            &self,
            channel_id: measurement_vo::ChannelId,
        ) -> Result<Vec<Measurement>, DomainError> {
            Ok(self
                .measurements
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m.channel_id.0 == channel_id.0)
                .cloned()
                .collect())
        }

        fn find_page(
            &self,
            channel_id: Option<measurement_vo::ChannelId>,
            offset: usize,
            limit: usize,
        ) -> Result<Vec<Measurement>, DomainError> {
            let mut measurements: Vec<Measurement> = self
                .measurements
                .lock()
                .unwrap()
                .iter()
                .filter(|m| channel_id.is_none_or(|id| m.channel_id.0 == id.0))
                .cloned()
                .collect();
            measurements.sort_by_key(|a| std::cmp::Reverse(a.timestamp.0));
            Ok(measurements.into_iter().skip(offset).take(limit).collect())
        }

        fn sum(
            &self,
            from: chrono::DateTime<chrono::Utc>,
            to: chrono::DateTime<chrono::Utc>,
            channel_ids: &[measurement_vo::ChannelId],
            _resolution_seconds: Option<i64>,
        ) -> Result<i64, DomainError> {
            Ok(self
                .measurements
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m.timestamp.0 >= from && m.timestamp.0 <= to)
                .filter(|m| channel_ids.contains(&m.channel_id))
                .map(|m| m.value.0)
                .sum())
        }

        fn sum_buckets(
            &self,
            _from: chrono::DateTime<chrono::Utc>,
            _to: chrono::DateTime<chrono::Utc>,
            _granularity: crate::core::domain::measurements::repository_port::BucketGranularity,
            _origin: chrono::DateTime<chrono::Utc>,
            _timezone: &str,
            _channel_ids: &[measurement_vo::ChannelId],
            _resolution_seconds: Option<i64>,
        ) -> Result<Vec<crate::core::domain::measurements::repository_port::TimeBucket>, DomainError>
        {
            Ok(Vec::new())
        }

        fn sum_buckets_by_channel(
            &self,
            _from: chrono::DateTime<chrono::Utc>,
            _to: chrono::DateTime<chrono::Utc>,
            _granularity: crate::core::domain::measurements::repository_port::BucketGranularity,
            _origin: chrono::DateTime<chrono::Utc>,
            _timezone: &str,
            _channel_ids: &[measurement_vo::ChannelId],
            _resolution_seconds: Option<i64>,
        ) -> Result<
            Vec<crate::core::domain::measurements::repository_port::ChannelBucket>,
            DomainError,
        > {
            Ok(Vec::new())
        }

        fn sum_weekdays(
            &self,
            _from: chrono::DateTime<chrono::Utc>,
            _to: chrono::DateTime<chrono::Utc>,
            _timezone: &str,
            _channel_ids: &[measurement_vo::ChannelId],
            _resolution_seconds: Option<i64>,
        ) -> Result<
            Vec<crate::core::domain::measurements::repository_port::WeekdayTotal>,
            DomainError,
        > {
            Ok(Vec::new())
        }

        fn sum_hours(
            &self,
            _from: chrono::DateTime<chrono::Utc>,
            _to: chrono::DateTime<chrono::Utc>,
            _timezone: &str,
            _channel_ids: &[measurement_vo::ChannelId],
            _resolution_seconds: Option<i64>,
        ) -> Result<Vec<crate::core::domain::measurements::repository_port::HourTotal>, DomainError>
        {
            Ok(Vec::new())
        }

        fn sum_hours_by_channel(
            &self,
            _from: chrono::DateTime<chrono::Utc>,
            _to: chrono::DateTime<chrono::Utc>,
            _timezone: &str,
            _channel_ids: &[measurement_vo::ChannelId],
            _resolution_seconds: Option<i64>,
        ) -> Result<
            Vec<crate::core::domain::measurements::repository_port::ChannelHourTotal>,
            DomainError,
        > {
            Ok(Vec::new())
        }

        fn sum_by_channel(
            &self,
            _from: chrono::DateTime<chrono::Utc>,
            _to: chrono::DateTime<chrono::Utc>,
            _channel_ids: &[measurement_vo::ChannelId],
            _resolution_seconds: Option<i64>,
        ) -> Result<
            Vec<crate::core::domain::measurements::repository_port::ChannelTotal>,
            DomainError,
        > {
            Ok(Vec::new())
        }

        fn sum_by_month(
            &self,
            _timezone: &str,
            _channel_ids: &[measurement_vo::ChannelId],
            _resolution_seconds: Option<i64>,
        ) -> Result<Vec<crate::core::domain::measurements::repository_port::MonthTotal>, DomainError>
        {
            Ok(Vec::new())
        }

        fn latest_by_channel(
            &self,
            channel_ids: &[measurement_vo::ChannelId],
        ) -> Result<
            Vec<crate::core::domain::measurements::repository_port::ChannelLatest>,
            DomainError,
        > {
            let measurements = self.measurements.lock().unwrap();
            let mut latest = Vec::new();
            for channel_id in channel_ids {
                if let Some(timestamp) = measurements
                    .iter()
                    .filter(|m| m.channel_id == *channel_id)
                    .map(|m| m.timestamp.0)
                    .max()
                {
                    latest.push(
                        crate::core::domain::measurements::repository_port::ChannelLatest {
                            channel_id: channel_id.0,
                            timestamp,
                        },
                    );
                }
            }
            Ok(latest)
        }
    }

    fn empty_repos() -> (
        Arc<MockCountingStationRepository>,
        Arc<MockChannelRepository>,
        Arc<MockMeasurementRepository>,
    ) {
        (
            Arc::new(MockCountingStationRepository {
                stations: Mutex::new(Vec::new()),
            }),
            Arc::new(MockChannelRepository {
                channels: Mutex::new(Vec::new()),
            }),
            Arc::new(MockMeasurementRepository {
                measurements: Mutex::new(Vec::new()),
            }),
        )
    }

    #[test]
    fn update_saves_stations_channels_and_measurements() {
        let t0 = timestamp("2024-01-01T00:00:00Z");
        let provider = Arc::new(SourceMockProvider {
            stations: vec![station_record("station-1")],
            channels: vec![channel_record("channel-1", "station-1")],
            batches: Mutex::new(VecDeque::from([source_batch(
                vec![
                    ("channel-1", measurement_record(1, t0)),
                    ("channel-1", measurement_record(1, t0)),
                    ("channel-1", measurement_record(1, t0)),
                ],
                Some(t0),
                false,
            )])),
            recorded_from: Mutex::new(Vec::new()),
            batch_size: 500,
        });
        let (station_repo, channel_repo, measurement_repo) = empty_repos();

        let service = DataImportService::new(
            station_repo.clone(),
            channel_repo.clone(),
            measurement_repo.clone(),
            Vec::new(),
        );

        let update = service
            .update_data_source(&runtime(provider.clone()), None, None, |_, _, _| Ok(()))
            .expect("update should succeed");

        assert!(update.completed);
        assert_eq!(update.processed_measurements, 3);
        assert_eq!(update.added_measurements, 3);
        assert_eq!(update.last_measurement_timestamp, Some(t0));

        assert_eq!(station_repo.stations.lock().unwrap().len(), 1);
        assert_eq!(channel_repo.channels.lock().unwrap().len(), 1);
        assert_eq!(measurement_repo.measurements.lock().unwrap().len(), 3);

        // The saved station must be linked to the importing data source.
        let saved_station = &station_repo.stations.lock().unwrap()[0];
        assert_eq!(
            saved_station.data_source_id.map(|id| id.0),
            Some(DataSource::id_from_name("Münster"))
        );
    }

    #[test]
    fn update_does_not_duplicate_already_known_stations_and_channels() {
        let station = station("station-1");
        let channel = channel("channel-1");
        let provider = Arc::new(SourceMockProvider {
            stations: vec![station_record("station-1")],
            channels: vec![channel_record("channel-1", "station-1")],
            batches: Mutex::new(VecDeque::from([source_batch(Vec::new(), None, false)])),
            recorded_from: Mutex::new(Vec::new()),
            batch_size: 500,
        });
        // The repository already knows both entities by their external id.
        let station_repo = Arc::new(MockCountingStationRepository {
            stations: Mutex::new(vec![station]),
        });
        let channel_repo = Arc::new(MockChannelRepository {
            channels: Mutex::new(vec![channel]),
        });

        let service = DataImportService::new(
            station_repo.clone(),
            channel_repo.clone(),
            Arc::new(MockMeasurementRepository {
                measurements: Mutex::new(Vec::new()),
            }),
            Vec::new(),
        );

        let update = service
            .update_data_source(&runtime(provider.clone()), None, None, |_, _, _| Ok(()))
            .expect("update should succeed");

        assert_eq!(update.processed_measurements, 0);
        assert_eq!(update.added_measurements, 0);
        assert_eq!(station_repo.stations.lock().unwrap().len(), 1);
        assert_eq!(channel_repo.channels.lock().unwrap().len(), 1);
    }

    #[test]
    fn resync_updates_existing_station_attributes_and_coordinates() {
        let existing = CountingStation {
            id: station_vo::Id(Uuid::from_u128(0x42)),
            name: station_vo::Name("Old Name".to_string()),
            description: station_vo::Description("desc".to_string()),
            external_datasource_id: Some(station_vo::ExternalDatasourceId("station-1".to_string())),
            data_source_id: None,
            coordinates: None,
            timezone: station_vo::Timezone("Europe/Berlin".to_string()),
            image_asset_id: None,
            image_sha256: None,
            status: station_vo::Status::Inactive,
        };
        let station_repo = Arc::new(MockCountingStationRepository {
            stations: Mutex::new(vec![existing]),
        });
        let provider = Arc::new(SourceMockProvider {
            stations: vec![CountingStationRecord {
                external_id: "station-1".to_string(),
                name: "Station 1".to_string(),
                description: "desc".to_string(),
                latitude: Some(51.96),
                longitude: Some(7.63),
                timezone: "Europe/Berlin".to_string(),
                image_sha256: None,
            }],
            channels: Vec::new(),
            batches: Mutex::new(VecDeque::from([source_batch(Vec::new(), None, false)])),
            recorded_from: Mutex::new(Vec::new()),
            batch_size: 500,
        });
        let service = DataImportService::new(
            station_repo.clone(),
            Arc::new(MockChannelRepository {
                channels: Mutex::new(Vec::new()),
            }),
            Arc::new(MockMeasurementRepository {
                measurements: Mutex::new(Vec::new()),
            }),
            Vec::new(),
        );

        service
            .update_data_source(&runtime(provider.clone()), None, None, |_, _, _| Ok(()))
            .expect("update should succeed");

        // The station already exists: the resync must not add a duplicate, but
        // must refresh its name and coordinates.
        let stations = station_repo.stations.lock().unwrap();
        assert_eq!(stations.len(), 1);
        assert_eq!(stations[0].name.0, "Station 1");
        assert_eq!(
            stations[0].coordinates,
            Some(station_vo::GeoCoordinates {
                latitude: 51.96,
                longitude: 7.63,
            })
        );
        // The station reappeared in the provider output, so it is reactivated.
        assert_eq!(stations[0].status, station_vo::Status::Active);
    }

    #[test]
    fn marks_stations_missing_from_the_provider_output_as_inactive() {
        // "station-1" is already known and linked to the Münster data source,
        // but the provider's current output only contains "station-2".
        let source_id = DataSource::id_from_name("Münster");
        let existing = CountingStation {
            id: station_vo::Id(Uuid::from_u128(0x51)),
            name: station_vo::Name("Station 1".to_string()),
            description: station_vo::Description("desc".to_string()),
            external_datasource_id: Some(station_vo::ExternalDatasourceId("station-1".to_string())),
            data_source_id: Some(station_vo::DataSourceId(source_id)),
            coordinates: None,
            timezone: station_vo::Timezone("Europe/Berlin".to_string()),
            image_asset_id: None,
            image_sha256: None,
            status: station_vo::Status::Active,
        };
        let station_repo = Arc::new(MockCountingStationRepository {
            stations: Mutex::new(vec![existing]),
        });
        let provider = Arc::new(SourceMockProvider {
            stations: vec![station_record("station-2")],
            channels: Vec::new(),
            batches: Mutex::new(VecDeque::from([source_batch(Vec::new(), None, false)])),
            recorded_from: Mutex::new(Vec::new()),
            batch_size: 500,
        });
        let service = DataImportService::new(
            station_repo.clone(),
            Arc::new(MockChannelRepository {
                channels: Mutex::new(Vec::new()),
            }),
            Arc::new(MockMeasurementRepository {
                measurements: Mutex::new(Vec::new()),
            }),
            Vec::new(),
        );

        service
            .update_data_source(&runtime(provider.clone()), None, None, |_, _, _| Ok(()))
            .expect("update should succeed");

        let stations = station_repo.stations.lock().unwrap();
        assert_eq!(stations.len(), 2);
        let station_1 = stations
            .iter()
            .find(|s| s.external_datasource_id.as_ref().map(|e| e.0.as_str()) == Some("station-1"))
            .unwrap();
        assert_eq!(
            station_1.status,
            station_vo::Status::Inactive,
            "a station the provider no longer serves must be marked inactive"
        );
        let station_2 = stations
            .iter()
            .find(|s| s.external_datasource_id.as_ref().map(|e| e.0.as_str()) == Some("station-2"))
            .unwrap();
        // Station 2 is newly imported but has no current data (the provider
        // serves no channels/measurements), so the stale check marks it inactive.
        assert_eq!(station_2.status, station_vo::Status::Inactive);
    }

    #[test]
    fn reactivates_a_station_that_reappears_in_the_provider_output() {
        // The station was inactive but the provider serves it again AND it has
        // current data — only then does it become active again.
        let source_id = DataSource::id_from_name("Münster");
        let existing = CountingStation {
            id: station_vo::Id(Uuid::from_u128(0x52)),
            name: station_vo::Name("Old Name".to_string()),
            description: station_vo::Description("desc".to_string()),
            external_datasource_id: Some(station_vo::ExternalDatasourceId("station-1".to_string())),
            data_source_id: Some(station_vo::DataSourceId(source_id)),
            coordinates: None,
            timezone: station_vo::Timezone("Europe/Berlin".to_string()),
            image_asset_id: None,
            image_sha256: None,
            status: station_vo::Status::Inactive,
        };
        let station_repo = Arc::new(MockCountingStationRepository {
            stations: Mutex::new(vec![existing]),
        });
        let current = chrono::Utc::now() - chrono::Duration::hours(1);
        let provider = Arc::new(SourceMockProvider {
            stations: vec![station_record("station-1")],
            channels: vec![channel_record("channel-1", "station-1")],
            batches: Mutex::new(VecDeque::from([source_batch(
                vec![("channel-1", measurement_record(1, current))],
                Some(current),
                false,
            )])),
            recorded_from: Mutex::new(Vec::new()),
            batch_size: 500,
        });
        let service = DataImportService::new(
            station_repo.clone(),
            Arc::new(MockChannelRepository {
                channels: Mutex::new(Vec::new()),
            }),
            Arc::new(MockMeasurementRepository {
                measurements: Mutex::new(Vec::new()),
            }),
            Vec::new(),
        );

        service
            .update_data_source(&runtime(provider.clone()), None, None, |_, _, _| Ok(()))
            .expect("update should succeed");

        let stations = station_repo.stations.lock().unwrap();
        assert_eq!(stations.len(), 1);
        assert_eq!(
            stations[0].status,
            station_vo::Status::Active,
            "a station that reappears with current data is reactivated"
        );
    }

    #[test]
    fn marks_a_station_inactive_when_it_has_no_current_data() {
        // The provider still serves the station, but its latest measurement is
        // older than the staleness window — the station is decommissioned.
        let source_id = DataSource::id_from_name("Münster");
        let existing = CountingStation {
            id: station_vo::Id(Uuid::from_u128(0x53)),
            name: station_vo::Name("Station 1".to_string()),
            description: station_vo::Description("desc".to_string()),
            external_datasource_id: Some(station_vo::ExternalDatasourceId("station-1".to_string())),
            data_source_id: Some(station_vo::DataSourceId(source_id)),
            coordinates: None,
            timezone: station_vo::Timezone("Europe/Berlin".to_string()),
            image_asset_id: None,
            image_sha256: None,
            status: station_vo::Status::Active,
        };
        let station_repo = Arc::new(MockCountingStationRepository {
            stations: Mutex::new(vec![existing]),
        });
        let stale = timestamp("2024-01-01T00:00:00Z");
        let provider = Arc::new(SourceMockProvider {
            stations: vec![station_record("station-1")],
            channels: vec![channel_record("channel-1", "station-1")],
            batches: Mutex::new(VecDeque::from([source_batch(
                vec![("channel-1", measurement_record(1, stale))],
                Some(stale),
                false,
            )])),
            recorded_from: Mutex::new(Vec::new()),
            batch_size: 500,
        });
        let service = DataImportService::new(
            station_repo.clone(),
            Arc::new(MockChannelRepository {
                channels: Mutex::new(Vec::new()),
            }),
            Arc::new(MockMeasurementRepository {
                measurements: Mutex::new(Vec::new()),
            }),
            Vec::new(),
        );

        service
            .update_data_source(&runtime(provider.clone()), None, None, |_, _, _| Ok(()))
            .expect("update should succeed");

        let stations = station_repo.stations.lock().unwrap();
        assert_eq!(stations.len(), 1);
        assert_eq!(
            stations[0].status,
            station_vo::Status::Inactive,
            "a served station without current data is marked inactive"
        );
    }

    #[test]
    fn update_resolves_measurements_to_the_persisted_channel_ids() {
        // Stations and channels are synced before measurements: the persisted
        // measurement must carry the created channel's id, proving the strict
        // stations -> channels -> measurements ordering.
        let t0 = timestamp("2024-01-01T00:00:00Z");
        let provider = Arc::new(SourceMockProvider {
            stations: vec![station_record("station-1")],
            channels: vec![channel_record("channel-1", "station-1")],
            batches: Mutex::new(VecDeque::from([source_batch(
                vec![("channel-1", measurement_record(1, t0))],
                Some(t0),
                false,
            )])),
            recorded_from: Mutex::new(Vec::new()),
            batch_size: 500,
        });
        let (station_repo, channel_repo, measurement_repo) = empty_repos();
        let service = DataImportService::new(
            station_repo.clone(),
            channel_repo.clone(),
            measurement_repo.clone(),
            Vec::new(),
        );

        let update = service
            .update_data_source(&runtime(provider.clone()), None, None, |_, _, _| Ok(()))
            .expect("update should succeed");

        assert!(update.completed);
        let saved_channel = &channel_repo.channels.lock().unwrap()[0];
        let saved_measurement = &measurement_repo.measurements.lock().unwrap()[0];
        assert_eq!(saved_measurement.channel_id.0, saved_channel.id.0);
        assert_eq!(
            saved_channel.counting_station_id.0,
            station_repo.stations.lock().unwrap()[0].id.0
        );
    }

    #[test]
    fn source_update_groups_multichannel_pages_per_channel() {
        let t0 = timestamp("2024-01-01T00:00:00Z");
        let provider = Arc::new(SourceMockProvider {
            stations: vec![station_record("station-1"), station_record("station-2")],
            channels: vec![
                channel_record("channel-1", "station-1"),
                channel_record("channel-2", "station-2"),
            ],
            // A single source page may carry measurements of several channels.
            batches: Mutex::new(VecDeque::from([source_batch(
                vec![
                    ("channel-1", measurement_record(1, t0)),
                    ("channel-2", measurement_record(1, t0)),
                    ("channel-2", measurement_record(1, t0)),
                ],
                Some(t0),
                false,
            )])),
            recorded_from: Mutex::new(Vec::new()),
            batch_size: 500,
        });
        let (_, channel_repo, measurement_repo) = empty_repos();
        let service = DataImportService::new(
            Arc::new(MockCountingStationRepository {
                stations: Mutex::new(Vec::new()),
            }),
            channel_repo.clone(),
            measurement_repo.clone(),
            Vec::new(),
        );

        let update = service
            .update_data_source(&runtime(provider.clone()), None, None, |_, _, _| Ok(()))
            .expect("update should succeed");

        assert_eq!(update.processed_measurements, 3);
        assert_eq!(update.added_measurements, 3);

        // One measurement is stored per source measurement, grouped by channel.
        let measurements = measurement_repo.measurements.lock().unwrap();
        assert_eq!(measurements.len(), 3);
        let channels = channel_repo.channels.lock().unwrap();
        let channel_1 = channels
            .iter()
            .find(|c| c.external_datasource_id.as_ref().map(|e| e.0.as_str()) == Some("channel-1"))
            .unwrap();
        let channel_2 = channels
            .iter()
            .find(|c| c.external_datasource_id.as_ref().map(|e| e.0.as_str()) == Some("channel-2"))
            .unwrap();
        let count_for = |id: &Uuid| {
            measurements
                .iter()
                .filter(|m| m.channel_id.0 == *id)
                .count()
        };
        assert_eq!(count_for(&channel_1.id.0), 1);
        assert_eq!(count_for(&channel_2.id.0), 2);
    }

    #[test]
    fn source_update_checkpoints_the_watermark_and_stops_at_the_deadline() {
        let t0 = timestamp("2024-01-01T00:00:00Z");
        let t1 = timestamp("2024-01-01T10:00:00Z");
        let provider = Arc::new(SourceMockProvider {
            stations: vec![station_record("station-1")],
            channels: vec![channel_record("channel-1", "station-1")],
            batches: Mutex::new(VecDeque::from([
                source_batch(
                    vec![("channel-1", measurement_record(1, t0))],
                    Some(t0),
                    true,
                ),
                source_batch(
                    vec![("channel-1", measurement_record(1, t1))],
                    Some(t1),
                    false,
                ),
            ])),
            recorded_from: Mutex::new(Vec::new()),
            batch_size: 100,
        });
        let (station_repo, _, _) = empty_repos();
        let service = DataImportService::new(
            station_repo.clone(),
            Arc::new(MockChannelRepository {
                channels: Mutex::new(Vec::new()),
            }),
            Arc::new(MockMeasurementRepository {
                measurements: Mutex::new(Vec::new()),
            }),
            Vec::new(),
        );

        let checkpoints = Arc::new(Mutex::new(Vec::new()));
        let update = service
            .update_data_source(
                &runtime(provider.clone()),
                None,
                None,
                |_processed, _added, watermark| {
                    checkpoints.lock().unwrap().push(watermark);
                    Ok(())
                },
            )
            .expect("source update should succeed");

        assert_eq!(update.processed_measurements, 2);
        assert_eq!(update.added_measurements, 2);
        assert!(update.completed);
        assert_eq!(update.last_measurement_timestamp, Some(t1));
        assert_eq!(
            update.first_measurement_timestamp,
            Some(t0),
            "the run reports its earliest processed measurement"
        );
        assert_eq!(
            update.last_measurement_timestamp_bound,
            Some(t1),
            "the run reports its latest processed measurement"
        );
        assert_eq!(
            *checkpoints.lock().unwrap(),
            vec![Some(t0), Some(t1)],
            "the watermark is checkpointed after every batch"
        );
    }

    #[test]
    fn source_update_stops_gracefully_at_the_deadline_without_marking_stale() {
        // A deadline-stop returns completed: false, the watermark is
        // checkpointed and the post-import stale pass is skipped.
        let t0 = timestamp("2024-01-01T00:00:00Z");
        let provider = Arc::new(SourceMockProvider {
            stations: vec![station_record("station-1")],
            channels: vec![channel_record("channel-1", "station-1")],
            batches: Mutex::new(VecDeque::from([source_batch(
                vec![("channel-1", measurement_record(1, t0))],
                Some(t0),
                true,
            )])),
            recorded_from: Mutex::new(Vec::new()),
            batch_size: 100,
        });
        let station_repo = Arc::new(MockCountingStationRepository {
            stations: Mutex::new(Vec::new()),
        });
        let channel_repo = Arc::new(MockChannelRepository {
            channels: Mutex::new(Vec::new()),
        });
        let service = DataImportService::new(
            station_repo.clone(),
            channel_repo.clone(),
            Arc::new(MockMeasurementRepository {
                measurements: Mutex::new(Vec::new()),
            }),
            Vec::new(),
        );

        // The deadline already passed: after the first batch the loop stops.
        let past = Utc::now() - chrono::Duration::seconds(1);
        let update = service
            .update_data_source(
                &runtime(provider.clone()),
                None,
                Some(past),
                |_processed, _added, watermark| {
                    if let Some(watermark) = watermark {
                        // The caller would normally persist the checkpoint.
                        assert_eq!(watermark, t0);
                    }
                    Ok(())
                },
            )
            .expect("source update should succeed");

        assert!(!update.completed, "a deadline stop is not a completed run");
        assert_eq!(update.processed_measurements, 1);
        assert_eq!(update.last_measurement_timestamp, Some(t0));

        // The stale pass must not run on a partial run: the station (no current
        // data, but created just now) is left Active.
        assert_eq!(
            station_repo.stations.lock().unwrap()[0].status,
            station_vo::Status::Active,
            "a deadline-stopped run must not decommission stations"
        );
    }

    #[test]
    fn source_update_reports_progress_and_resumes_from_imported_until() {
        let t1 = timestamp("2024-01-01T10:00:00Z");
        let last_updated = timestamp("2024-01-01T09:00:00Z");
        let provider = Arc::new(SourceMockProvider {
            stations: vec![station_record("station-1")],
            channels: vec![channel_record("channel-1", "station-1")],
            batches: Mutex::new(VecDeque::from([
                source_batch(
                    vec![
                        ("channel-1", measurement_record(1, t1)),
                        ("channel-1", measurement_record(1, t1)),
                    ],
                    Some(t1),
                    true,
                ),
                source_batch(
                    vec![("channel-1", measurement_record(1, t1))],
                    Some(t1),
                    false,
                ),
            ])),
            recorded_from: Mutex::new(Vec::new()),
            batch_size: 500,
        });
        let service = DataImportService::new(
            Arc::new(MockCountingStationRepository {
                stations: Mutex::new(Vec::new()),
            }),
            Arc::new(MockChannelRepository {
                channels: Mutex::new(Vec::new()),
            }),
            Arc::new(MockMeasurementRepository {
                measurements: Mutex::new(Vec::new()),
            }),
            Vec::new(),
        );

        let progress = Arc::new(Mutex::new(Vec::new()));
        let update = service
            .update_data_source(
                &runtime(provider.clone()),
                Some(last_updated),
                None,
                |processed, added, _watermark| {
                    progress.lock().unwrap().push((processed, added));
                    Ok(())
                },
            )
            .expect("update should succeed");

        assert_eq!(update.processed_measurements, 3);
        assert_eq!(update.added_measurements, 3);
        assert_eq!(update.last_measurement_timestamp, Some(t1));
        assert_eq!(*progress.lock().unwrap(), vec![(2, 2), (3, 3)]);

        // Every source-level call is anchored at the data source's imported_until
        // (the provider's own scanner reseeds only when the anchor changes).
        let recorded = provider.recorded_from.lock().unwrap();
        assert_eq!(recorded.len(), 2);
        assert!(
            recorded.iter().all(|from| *from == Some(last_updated)),
            "the run must resume from the persisted imported_until"
        );
    }

    #[test]
    fn source_update_leaves_the_cursor_at_none_without_a_watermark() {
        let provider = Arc::new(SourceMockProvider {
            stations: vec![station_record("station-1")],
            channels: vec![channel_record("channel-1", "station-1")],
            batches: Mutex::new(VecDeque::from([source_batch(Vec::new(), None, false)])),
            recorded_from: Mutex::new(Vec::new()),
            batch_size: 500,
        });
        let service = DataImportService::new(
            Arc::new(MockCountingStationRepository {
                stations: Mutex::new(Vec::new()),
            }),
            Arc::new(MockChannelRepository {
                channels: Mutex::new(Vec::new()),
            }),
            Arc::new(MockMeasurementRepository {
                measurements: Mutex::new(Vec::new()),
            }),
            Vec::new(),
        );

        let update = service
            .update_data_source(&runtime(provider.clone()), None, None, |_, _, _| Ok(()))
            .expect("update should succeed");

        assert!(update.completed);
        assert_eq!(update.processed_measurements, 0);
        assert_eq!(update.last_measurement_timestamp, None);
    }

    #[test]
    fn update_rejects_channel_referencing_unknown_station() {
        let provider = Arc::new(SourceMockProvider {
            stations: vec![station_record("station-1")],
            channels: vec![channel_record("channel-1", "station-99")],
            batches: Mutex::new(VecDeque::new()),
            recorded_from: Mutex::new(Vec::new()),
            batch_size: 500,
        });
        let service = DataImportService::new(
            Arc::new(MockCountingStationRepository {
                stations: Mutex::new(Vec::new()),
            }),
            Arc::new(MockChannelRepository {
                channels: Mutex::new(Vec::new()),
            }),
            Arc::new(MockMeasurementRepository {
                measurements: Mutex::new(Vec::new()),
            }),
            Vec::new(),
        );

        let error = service
            .update_data_source(&runtime(provider.clone()), None, None, |_, _, _| Ok(()))
            .expect_err("a channel referencing an unknown station must fail");
        assert!(matches!(error, DomainError::InvalidQuery(_)));
    }

    #[test]
    fn update_rejects_measurement_for_unknown_channel() {
        let provider = Arc::new(SourceMockProvider {
            stations: vec![station_record("station-1")],
            channels: vec![channel_record("channel-1", "station-1")],
            batches: Mutex::new(VecDeque::from([source_batch(
                vec![(
                    "no-such-channel",
                    measurement_record(1, timestamp("2024-01-01T00:00:00Z")),
                )],
                None,
                false,
            )])),
            recorded_from: Mutex::new(Vec::new()),
            batch_size: 500,
        });
        let service = DataImportService::new(
            Arc::new(MockCountingStationRepository {
                stations: Mutex::new(Vec::new()),
            }),
            Arc::new(MockChannelRepository {
                channels: Mutex::new(Vec::new()),
            }),
            Arc::new(MockMeasurementRepository {
                measurements: Mutex::new(Vec::new()),
            }),
            Vec::new(),
        );

        let error = service
            .update_data_source(&runtime(provider.clone()), None, None, |_, _, _| Ok(()))
            .expect_err("a measurement for an unknown channel must fail");
        assert!(matches!(error, DomainError::InvalidQuery(_)));
    }

    #[test]
    fn update_links_station_to_builtin_default_when_provider_has_no_image() {
        let provider = Arc::new(ImageProvider {
            stations: vec![station_record("station-1")], // image_sha256: None
            images: Mutex::new(HashMap::new()),
            fetch_calls: Mutex::new(0),
        });
        let assets = image_service();
        let service = DataImportService::new(
            Arc::new(MockCountingStationRepository {
                stations: Mutex::new(Vec::new()),
            }),
            Arc::new(MockChannelRepository {
                channels: Mutex::new(Vec::new()),
            }),
            Arc::new(MockMeasurementRepository {
                measurements: Mutex::new(Vec::new()),
            }),
            Vec::new(),
        )
        .with_asset_service(assets.clone());

        service
            .update_data_source(&runtime(provider.clone()), None, None, |_, _, _| Ok(()))
            .expect("update should succeed");

        let station = service
            .counting_station_repository
            .find_by_external_datasource_id(station_vo::ExternalDatasourceId(
                "station-1".to_string(),
            ))
            .unwrap()
            .expect("station must be persisted");
        assert_eq!(station.image_asset_id, Some(default_asset().id));
        assert_eq!(station.image_sha256, None);
        assert_eq!(*assets.stored_provider.lock().unwrap(), 0);
    }

    #[test]
    fn update_stores_provider_image_when_hash_changes() {
        let hash = "a".repeat(64);
        let provider = Arc::new(ImageProvider {
            stations: vec![CountingStationRecord {
                external_id: "station-1".to_string(),
                name: "Station 1".to_string(),
                description: "desc".to_string(),
                latitude: Some(51.96),
                longitude: Some(7.63),
                timezone: "Europe/Berlin".to_string(),
                image_sha256: Some(hash.clone()),
            }],
            images: Mutex::new(HashMap::from([(
                "station-1".to_string(),
                StationImage {
                    sha256: hash.clone(),
                    content_type: "image/jpeg".to_string(),
                    bytes: vec![1, 2, 3],
                },
            )])),
            fetch_calls: Mutex::new(0),
        });
        let assets = image_service();
        let service = DataImportService::new(
            Arc::new(MockCountingStationRepository {
                stations: Mutex::new(Vec::new()),
            }),
            Arc::new(MockChannelRepository {
                channels: Mutex::new(Vec::new()),
            }),
            Arc::new(MockMeasurementRepository {
                measurements: Mutex::new(Vec::new()),
            }),
            Vec::new(),
        )
        .with_asset_service(assets.clone());

        service
            .update_data_source(&runtime(provider.clone()), None, None, |_, _, _| Ok(()))
            .expect("update should succeed");

        let station = service
            .counting_station_repository
            .find_by_external_datasource_id(station_vo::ExternalDatasourceId(
                "station-1".to_string(),
            ))
            .unwrap()
            .expect("station must be persisted");
        assert_eq!(station.image_sha256, Some(hash));
        assert!(
            station.image_asset_id.is_some(),
            "a provider asset must be linked"
        );
        assert_eq!(*assets.stored_provider.lock().unwrap(), 1);
    }

    #[test]
    fn update_skips_image_fetch_when_hash_is_unchanged() {
        // The existing station already has the provider's hash persisted.
        let mut existing = station("station-1");
        existing.image_sha256 = Some("hash1".to_string());
        existing.image_asset_id = Some(default_asset().id);
        let provider = Arc::new(ImageProvider {
            stations: vec![CountingStationRecord {
                external_id: "station-1".to_string(),
                name: "Station 1".to_string(),
                description: "desc".to_string(),
                latitude: None,
                longitude: None,
                timezone: "Europe/Berlin".to_string(),
                image_sha256: Some("hash1".to_string()),
            }],
            images: Mutex::new(HashMap::new()),
            fetch_calls: Mutex::new(0),
        });
        let assets = image_service();
        let station_repo = Arc::new(MockCountingStationRepository {
            stations: Mutex::new(vec![existing]),
        });
        let service = DataImportService::new(
            station_repo.clone(),
            Arc::new(MockChannelRepository {
                channels: Mutex::new(Vec::new()),
            }),
            Arc::new(MockMeasurementRepository {
                measurements: Mutex::new(Vec::new()),
            }),
            Vec::new(),
        )
        .with_asset_service(assets.clone());

        service
            .update_data_source(&runtime(provider.clone()), None, None, |_, _, _| Ok(()))
            .expect("update should succeed");

        assert_eq!(
            *provider.fetch_calls.lock().unwrap(),
            0,
            "unchanged hash must not fetch the image bytes"
        );
        assert_eq!(*assets.stored_provider.lock().unwrap(), 0);
        assert_eq!(
            station_repo.stations.lock().unwrap()[0].image_sha256,
            Some("hash1".to_string())
        );
    }

    #[test]
    fn update_falls_back_to_default_when_provider_has_no_image_bytes() {
        // The provider reports a hash but serves no image bytes for it: the
        // station falls back to the built-in default and the hash is not
        // recorded (so a later sync can retry).
        let hash = "a".repeat(64);
        let provider = Arc::new(ImageProvider {
            stations: vec![CountingStationRecord {
                external_id: "station-1".to_string(),
                name: "Station 1".to_string(),
                description: "desc".to_string(),
                latitude: None,
                longitude: None,
                timezone: "Europe/Berlin".to_string(),
                image_sha256: Some(hash),
            }],
            images: Mutex::new(HashMap::new()), // no bytes served for the hash
            fetch_calls: Mutex::new(0),
        });
        let assets = image_service();
        let service = DataImportService::new(
            Arc::new(MockCountingStationRepository {
                stations: Mutex::new(Vec::new()),
            }),
            Arc::new(MockChannelRepository {
                channels: Mutex::new(Vec::new()),
            }),
            Arc::new(MockMeasurementRepository {
                measurements: Mutex::new(Vec::new()),
            }),
            Vec::new(),
        )
        .with_asset_service(assets.clone());

        service
            .update_data_source(&runtime(provider.clone()), None, None, |_, _, _| Ok(()))
            .expect("update should succeed");

        let station = service
            .counting_station_repository
            .find_by_external_datasource_id(station_vo::ExternalDatasourceId(
                "station-1".to_string(),
            ))
            .unwrap()
            .expect("station must be persisted");
        assert_eq!(station.image_asset_id, Some(default_asset().id));
        assert_eq!(station.image_sha256, None);
        assert_eq!(*provider.fetch_calls.lock().unwrap(), 1);
        assert_eq!(*assets.stored_provider.lock().unwrap(), 0);
    }

    #[test]
    fn update_reports_import_phases_in_order() {
        let t0 = timestamp("2024-01-01T00:00:00Z");
        let provider = Arc::new(SourceMockProvider {
            stations: vec![station_record("station-1")],
            channels: vec![channel_record("channel-1", "station-1")],
            batches: Mutex::new(VecDeque::from([source_batch(
                vec![("channel-1", measurement_record(1, t0))],
                Some(t0),
                false,
            )])),
            recorded_from: Mutex::new(Vec::new()),
            batch_size: 500,
        });
        let (station_repo, _, _) = empty_repos();
        let service = DataImportService::new(
            station_repo.clone(),
            Arc::new(MockChannelRepository {
                channels: Mutex::new(Vec::new()),
            }),
            Arc::new(MockMeasurementRepository {
                measurements: Mutex::new(Vec::new()),
            }),
            Vec::new(),
        );

        let phases = Arc::new(Mutex::new(Vec::new()));
        let phases_for_closure = phases.clone();
        let update = service
            .update_data_source_with_progress(
                &runtime(provider.clone()),
                None,
                None,
                |_, _, _| Ok(()),
                |phase| {
                    phases_for_closure.lock().unwrap().push(phase);
                    Ok(())
                },
            )
            .expect("update should succeed");

        assert!(update.completed);
        assert_eq!(
            *phases.lock().unwrap(),
            vec![
                DataSourceImportPhase::Starting,
                DataSourceImportPhase::SyncingStations,
                DataSourceImportPhase::SyncingChannels,
                DataSourceImportPhase::ImportingMeasurements,
            ],
            "the run reports every phase it passes through"
        );
    }
}
