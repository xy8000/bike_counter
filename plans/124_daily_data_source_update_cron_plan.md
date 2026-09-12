# 124 - Default data-source update cron once a day

Status: implemented

## Problem

The default `data_source_update_cron` is hourly (`"0 0 * * * *"`), so the
scheduled import job re-triggers every hour. On top of that, the tracked
template [`config.toml.example`](config.toml.example:13) was **inconsistent and
off**:

- its comment claimed `default: every hour`, but
- its sample value `"0 * * * * *"` (6-field `sec min hour dom mon dow`) actually
  means **every minute** — it is missing the hour field.

So the comment and the sample value disagreed with each other and with the code
default, and the example cron fired far more often than documented.

Running the import every hour is unnecessary for the mostly daily upstream
feeds and multiplies provider traffic; once a day is enough.

## Approach

Change the default to a single daily run at **03:00**, i.e. just before the
daily OpenData export (`DEFAULT_OPENDATA_EXPORT_CRON` = 03:30), so the export
sees the freshly imported previous-day data. Fix the comments/samples so the
documented value matches the code everywhere.

1. [`configuration.rs`](backend/src/core/domain/configuration/configuration.rs:9) —
   `DEFAULT_DATA_SOURCE_UPDATE_CRON` `"0 0 * * * *"` → `"0 0 3 * * *"` and update
   the doc comment to "once a day at 03:00".
2. [`config.toml.example`](config.toml.example:12) — fix the comment to
   "once a day at 03:00" and the sample value to `"0 0 3 * * *"`.
3. [`data_source_update_service.rs`](backend/src/core/application/data_source_update_service.rs:1516) —
   the `runs_when_last_run_is_overdue` test assumed an hourly cron (a two-hour-old
   finish was already overdue); use a two-day-old finish so the daily schedule has
   missed a trigger.
4. Keep the test-script configs consistent:
   [`docker-compose-test.sh`](scripts/docker-compose-test.sh:72) and
   [`e2e-playwright.sh`](scripts/e2e-playwright.sh:101).

## Notes

- The cron format is 6-field (`sec min hour day-of-month month day-of-week`),
  identical to the other job defaults (asset cleanup `0 0 4 * * *`, opendata
  export `0 30 3 * * *`).
- The user's gitignored [`config.toml`](config.toml:6) is an explicit local
  override and is intentionally left untouched.
- `run_if_due` still runs the job at startup when it has never succeeded, so a
  restart catches up regardless of the schedule.

## Definition of done

- [x] `DEFAULT_DATA_SOURCE_UPDATE_CRON` is `"0 0 3 * * *"` with a matching doc comment
- [x] [`config.toml.example`](config.toml.example:12) comment and value both say daily at 03:00
- [x] `runs_when_last_run_is_overdue` updated for the daily default
- [x] test-script config comments/values aligned
- [x] `make check` green
- [x] `make test-rest` green (plus targeted `data_source_update_service` 24 and `configuration` 51 tests)
- [x] committed on the current branch
