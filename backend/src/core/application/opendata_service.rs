//! Application service exposing the opendata file-registry reads (the index /
//! file endpoints of the REST tree) through the core. Thin read model over the
//! [`OpenDataFileRepository`]; the dataset metadata (incl. the JSON schemata) is
//! static and lives in the handler/DTO layer.

use std::sync::Arc;

use uuid::Uuid;

use crate::core::domain::error::DomainError;
use crate::core::domain::opendata::file::{Granularity, OpenDataFile};
use crate::core::domain::opendata::file_repository_port::OpenDataFileRepository;
use crate::core::domain::opendata::service_port::OpenDataServicePort;

pub struct OpenDataService {
    repository: Arc<dyn OpenDataFileRepository>,
}

impl OpenDataService {
    pub fn new(repository: Arc<dyn OpenDataFileRepository>) -> Self {
        Self { repository }
    }

    pub fn list_periods(
        &self,
        granularity: Granularity,
        station_id: Option<Uuid>,
    ) -> Result<Vec<String>, DomainError> {
        self.repository.list_periods(granularity, station_id)
    }

    pub fn list_files(
        &self,
        granularity: Granularity,
        period: &str,
        station_id: Option<Uuid>,
    ) -> Result<Vec<OpenDataFile>, DomainError> {
        self.repository
            .find_by_period(granularity, period, station_id)
    }

    pub fn find_file(&self, object_key: &str) -> Result<Option<OpenDataFile>, DomainError> {
        self.repository.find_by_object_key(object_key)
    }
}

impl OpenDataServicePort for OpenDataService {
    fn list_periods(
        &self,
        granularity: Granularity,
        station_id: Option<Uuid>,
    ) -> Result<Vec<String>, DomainError> {
        self.list_periods(granularity, station_id)
    }

    fn list_files(
        &self,
        granularity: Granularity,
        period: &str,
        station_id: Option<Uuid>,
    ) -> Result<Vec<OpenDataFile>, DomainError> {
        self.list_files(granularity, period, station_id)
    }

    fn find_file(&self, object_key: &str) -> Result<Option<OpenDataFile>, DomainError> {
        self.find_file(object_key)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use chrono::Utc;
    use uuid::Uuid;

    use super::*;
    use crate::core::domain::opendata::file::{Format, object_key};

    struct MemoryOpenDataFileRepository {
        files: Mutex<Vec<OpenDataFile>>,
    }

    impl MemoryOpenDataFileRepository {
        fn new(files: Vec<OpenDataFile>) -> Self {
            Self {
                files: Mutex::new(files),
            }
        }
    }

    impl OpenDataFileRepository for MemoryOpenDataFileRepository {
        fn insert(&self, file: &OpenDataFile) -> Result<(), DomainError> {
            self.files.lock().unwrap().push(file.clone());
            Ok(())
        }

        fn find_by_object_key(
            &self,
            object_key: &str,
        ) -> Result<Option<OpenDataFile>, DomainError> {
            Ok(self
                .files
                .lock()
                .unwrap()
                .iter()
                .find(|file| file.object_key == object_key)
                .cloned())
        }

        fn list_periods(
            &self,
            granularity: Granularity,
            station_id: Option<Uuid>,
        ) -> Result<Vec<String>, DomainError> {
            let mut periods: Vec<String> = self
                .files
                .lock()
                .unwrap()
                .iter()
                .filter(|file| file.granularity == granularity && file.station_id == station_id)
                .map(|file| file.period.clone())
                .collect();
            periods.sort_by(|a, b| b.cmp(a));
            periods.dedup();
            Ok(periods)
        }

        fn find_by_period(
            &self,
            granularity: Granularity,
            period: &str,
            station_id: Option<Uuid>,
        ) -> Result<Vec<OpenDataFile>, DomainError> {
            let mut files: Vec<OpenDataFile> = self
                .files
                .lock()
                .unwrap()
                .iter()
                .filter(|file| {
                    file.granularity == granularity
                        && file.period == period
                        && file.station_id == station_id
                })
                .cloned()
                .collect();
            files.sort_by(|a, b| a.format.as_str().cmp(b.format.as_str()));
            Ok(files)
        }

        fn max_period(
            &self,
            granularity: Granularity,
            station_id: Option<Uuid>,
        ) -> Result<Option<String>, DomainError> {
            Ok(self
                .list_periods(granularity, station_id)?
                .into_iter()
                .next())
        }
    }

    fn file(
        object_key: &str,
        granularity: Granularity,
        period: &str,
        format: Format,
        station_id: Option<Uuid>,
    ) -> OpenDataFile {
        OpenDataFile {
            id: Uuid::new_v4(),
            object_key: object_key.to_string(),
            station_id,
            granularity,
            period: period.to_string(),
            format,
            byte_size: 1,
            sha256: "a".repeat(64),
            created_at: Utc::now(),
        }
    }

    fn service() -> OpenDataService {
        let station = Uuid::from_u128(7);
        let files = vec![
            file(
                &object_key(None, Granularity::Daily, "2026-09-04", Format::Json),
                Granularity::Daily,
                "2026-09-04",
                Format::Json,
                None,
            ),
            file(
                &object_key(None, Granularity::Daily, "2026-09-05", Format::Parquet),
                Granularity::Daily,
                "2026-09-05",
                Format::Parquet,
                None,
            ),
            file(
                &object_key(None, Granularity::Daily, "2026-09-05", Format::CsvGz),
                Granularity::Daily,
                "2026-09-05",
                Format::CsvGz,
                None,
            ),
            file(
                &object_key(Some(station), Granularity::Monthly, "2026-09", Format::Json),
                Granularity::Monthly,
                "2026-09",
                Format::Json,
                Some(station),
            ),
        ];
        OpenDataService::new(Arc::new(MemoryOpenDataFileRepository::new(files)))
    }

    #[test]
    fn lists_periods_newest_first_for_a_scope() {
        let service = service();
        let periods = service.list_periods(Granularity::Daily, None).unwrap();
        assert_eq!(periods, vec!["2026-09-05", "2026-09-04"]);
    }

    #[test]
    fn lists_periods_of_a_station_scope() {
        let station = Uuid::from_u128(7);
        let service = service();
        let periods = service
            .list_periods(Granularity::Monthly, Some(station))
            .unwrap();
        assert_eq!(periods, vec!["2026-09"]);
        assert!(
            service
                .list_periods(Granularity::Daily, Some(station))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn lists_files_of_one_period_with_all_distributions() {
        let service = service();
        let files = service
            .list_files(Granularity::Daily, "2026-09-05", None)
            .unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].format, Format::CsvGz);
        assert_eq!(files[1].format, Format::Parquet);
    }

    #[test]
    fn find_file_by_object_key() {
        let service = service();
        let key = object_key(None, Granularity::Daily, "2026-09-05", Format::Parquet);
        assert!(service.find_file(&key).unwrap().is_some());
        assert!(
            service
                .find_file("opendata/missing.json")
                .unwrap()
                .is_none()
        );
    }
}
