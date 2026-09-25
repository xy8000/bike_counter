# 146 - Overdue data-source update not scheduled (diagnosis + guard)

Status: implemented

## Report

A running stack never refreshed its data sources, even though the last
successful import was ~12 days old and `data_source_update_cron="0 0 * * * *"`
(hourly) was configured. The backend log showed no scheduler activity at all.

## Root cause

`config.toml` (a **gitignored**, local developer file) contained:

```toml
scheduled_jobs_enabled = false
```

[`main.rs`](backend/src/main.rs:394) only spawns the data-source, asset-cleanup,
tiles and opendata schedulers (and the job watcher) when
`configuration.scheduled_jobs_enabled()` is `true`; otherwise it merely runs the
one-off measurement-rollup backfill. So no `run_if_due` was ever invoked and no
overdue job could start. The shipped template
([`config.toml.example`](config.toml.example:10)) already defaults to `true`.

The disabled flag was additionally *silent* — nothing in the log said that
scheduling was turned off.

## Tests written first (identify the issue)

Both tests pass, proving the scheduling code itself is correct and isolating the
cause to configuration:

- [`run_scheduler_invokes_the_job_once_at_startup`](backend/src/adapter/driving/job_scheduler.rs:71)
  — asserts the scheduler calls `ScheduledJobPort::run_if_due` immediately at
  startup (before any cron tick).
- [`starts_an_overdue_update_with_the_reported_hourly_cron`](backend/src/core/application/data_source_update_service.rs:1595)
  — mirrors the reported setup (hourly cron + a last success 12 days ago) and
  asserts a new job **is** started.

## Fix

- Restore `scheduled_jobs_enabled = true` in the local `config.toml`.
- Emit a startup `WARN` in [`main.rs`](backend/src/main.rs:426) when scheduled
  jobs are disabled, so this misconfiguration is never silent again.

## Definition of done

- [x] Diagnostic tests added at the scheduler and service boundaries
- [x] Root cause documented
- [x] Local `config.toml` re-enables scheduled jobs
- [x] Startup `WARN` when jobs are disabled
- [x] `make check` green
- [x] `make test` green (809 tests)
- [x] `make coverage` green (overall 87.03 %, core 97.16 %)
