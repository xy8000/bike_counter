# 161 - Job overview & average import runtime

Status: implemented

## Goal

GitHub issue #38: show the import jobs of each data source as a list-table and
the average import duration in the detail view, with Playwright coverage.

## Backend

- Migration [`V27__add_import_run_facts.sql`](backend/migrations/V27__add_import_run_facts.sql)
  adds `added_measurements`, `added_stations`, `warning_count`, `error_count` to
  `data_source_imports`.
- [`import_run.rs`](backend/src/core/domain/data_source/import_run.rs): new
  `ImportRunFacts` snapshot + `DataImportRun.facts`.
- [`import_run_port.rs`](backend/src/core/domain/data_source/import_run_port.rs):
  `finish_with_facts` / `fail_with_facts` (default: delegate to `finish`/`fail`)
  and `list_by_data_source` (default: latest only).
- [`import_run_repository.rs`](backend/src/adapter/driven/postgres/import_run_repository.rs):
  persists the facts and reads the full run history.
- [`data_import_service.rs`](backend/src/core/application/data_import_service.rs):
  `DataSourceUpdate` now reports `added_stations`; `sync_counting_stations`
  counts newly created stations.
- [`data_source_update_service.rs`](backend/src/core/application/data_source_update_service.rs):
  snapshots warning/error counts and finishes runs with the fact snapshot.
- New BFF endpoint `GET /api/bff/data-sources/{id}/imports` returning the run
  history plus `average_duration_seconds` (finished runs only).

## Frontend

- [`types.ts`](frontend/src/features/dataSources/types.ts): extend
  `DataSourceImport` and add `DataSourceImports`.
- [`api.ts`](frontend/src/features/dataSources/api.ts): `fetchDataSourceImports`.
- [`ImportHistory.tsx`](frontend/src/features/dataSources/ImportHistory.tsx):
  jobs table (start time, duration, success, warnings, errors, new
  measurements, new stations) + average duration.
- [`DataSourceDetail.tsx`](frontend/src/features/dataSources/DataSourceDetail.tsx):
  renders the history section.

## Definition of done

- [x] Migration + repository/domain/service changes
- [x] BFF endpoint + OpenAPI registration
- [x] Backend tests (`make test-rest` 130 green; `data_import_service` /
      `data_source_*` unit tests green; clippy green)
- [x] Frontend unit tests (576 green, including the new `ImportHistory` suite)
- [x] Playwright spec covers the jobs section on the detail page
