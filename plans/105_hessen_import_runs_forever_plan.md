# 105 — "Hessen is running for ages": stale RUNNING job after a backend restart

## Status: in progress / diagnosis (corrected)

> **Correction (v2):** an earlier version of this plan blamed the screen-scraper
> throughput for the multi-hour "Running" state. Direct measurement proved that
> wrong: each site page takes only ~2.1–2.4 s, so a full 549-station pass is
> **~20 minutes**, not 1 h+. The actual cause is a **stale `RUNNING`
> `data_source_update` job + per-source import run left behind by a backend
> process that was restarted mid-import**. The restarted process refuses to start
> a fresh run until the stale job's `lifetime_until` passes, so nothing imports
> and the UI keeps showing "Running · 1h 13m".

## Symptom

Hessen Mobil's data-source panel shows:

- "Data imported until 04.09.26, 00:00"
- "Last import: **Running · 1h 13m**"

and the `imported_until` watermark does not advance.

## Root cause (verified on the live stack, 2026-09-05)

1. **A backend restart cut an import in half.** Job `data_source_update`
   `2158b850…` was created at `14:28:43` and Hessen's per-source import run
   `34ac14e1…` at `14:29:13`. The backend container's process (PID 1) started
   **~14:55** (elapsed ~59 min at 15:54) — i.e. **the job/run belong to the
   previous process**, which was killed mid-import (e.g. by `docker compose up
   --build` / `restart`). The pre-restart process had already imported the
   current gap: 526/549 live channels now carry their 04.09 row.

2. **Nothing is importing now.** The restarted process's log since startup shows
   only
   `Data source update job 2158b850… is still running (until 16:28:43 UTC); skipping`
   repeated every cron tick — it has **never** started a fresh run (no "started",
   no "overdue", no "Expired"). `pg_stat_activity` shows the backend connections
   **idle**; there is no active measurement INSERT.

3. **Why it blocks.** [`DataSourceUpdateService::run_if_due`](backend/src/core/application/data_source_update_service.rs:92)
   expires RUNNING jobs only once they are **past** `lifetime_until`
   ([`expire_running_jobs`](backend/src/adapter/driven/postgres/job_repository.rs))
   and otherwise treats a RUNNING job as "still in progress" → skip
   ([line 110](backend/src/core/application/data_source_update_service.rs:110)).
   `lifetime_until = started + max_lifetime (7200 s)` → the stale job holds the
   whole `data_source_update` slot until **16:28:43**, so **every source**
   (not just Hessen) stops updating for ~1.5 h after the restart. The UI shows
   the orphaned import run (`status = RUNNING`, never finished) as "Running".

4. **This is a recurring pattern.** Orphaned `RUNNING` rows in
   `data_source_imports` from older restarts exist for **Hamburg back to
   2026-09-02** — per-source runs are only closed in-process
   ([`import_run_repository.finish`](backend/src/core/application/data_source_update_service.rs:383)),
   so rows from dead processes accumulate forever and can keep misleading the
   "Last import" UI.

### What is NOT the cause (corrected)

- **Scraper throughput / rate limiter.** The
  [`RateLimiter`](backend/src/adapter/driven/eco_counter/scraping/rate_limit.rs)
  only sleeps the *remainder* of the 1 s minimum interval when the previous
  request finished in < 1 s; a slow request adds **no** extra wait. Measured from
  inside the backend container, a site detail page is **~2.1–2.4 s**
  (`curl`, `RSC: 1`, year 2026): a full pass over all ~549 sites is **~20 min**,
  comfortably inside the 2 h job window.
- **A hung request without a timeout.** Requests are slow-but-bounded; the run
  was not wedged on a single request — it was never running at all after 14:55.

## Goals

1. After any backend restart, imports resume **immediately** (from the
   `imported_until` checkpoint) instead of being blocked for the stale job's
   remaining lifetime.
2. Never show a source as "Running" when its owning process is dead — orphaned
   `data_source_imports` rows must be closed.
3. Keep ShedLock-style mutual exclusion intact for genuinely concurrent workers.

## Recommended changes

### A. Abandoned-job recovery on startup (primary fix)
- The app is deployed **single-instance** (docker-compose), and per-batch
  `imported_until` checkpointing makes resumption lossless
  ([`data_source_update_service.rs`](backend/src/core/application/data_source_update_service.rs:321)).
  On startup (before the first `run_if_due`), **expire any RUNNING
  `data_source_update` job** whose `started_at` predates this process's boot —
  mark it `FAILED` (`max_lifetime_exceeded = true`, message e.g. "Abandoned on
  restart") — and **FAIL its RUNNING `data_source_imports` rows**. The next cron
  tick then starts a real run from the checkpoint.
- Simplest robust rule for a single instance: expire RUNNING jobs of this type
  older than the process start time. Guard behind a `scheduled_jobs_enabled`
  flag if multiple workers can ever run.

### B. Orphaned import-run reaper (keeps the UI honest)
- A small periodic sweep (e.g. in `run_if_due` or the scheduler) that FAILs any
  RUNNING `data_source_imports` row whose owning `job_id` is no longer RUNNING
  (or whose job was expired). This also cleans the historical Hamburg/Hessen
  ghosts and prevents a source from forever displaying its last (dead) run as
  "Running".

### C. Optional hardening of the scraper (secondary, not the observed cause)
- Add an explicit request timeout to
  [`HttpPageFetcher`](backend/src/adapter/driven/eco_counter/scraping/fetcher.rs)
  and, if desired, a bounded concurrent page loop in
  [`EcoCounterWebAdapter`](backend/src/adapter/driven/eco_counter/scraping/adapter.rs)
  to shrink the ~20 min pass further. Not required to fix this bug.

## Done when

- Restarting the backend while an import is RUNNING does not stall updates: the
  next cron tick starts a fresh run and `imported_until` advances again within a
  few minutes.
- No source ever shows "Running · Xh" when its process is dead; orphaned
  `data_source_imports` rows are FAILED/cleaned.
- Existing stale rows (Hessen `34ac14e1…`, Hamburg ghosts) are cleared.

## Notes

- Immediate unblock for the current stack (no code change): `docker compose
  restart backend` does **not** help (same logic) — the stale job must be
  expired. Options: wait until 16:28:43, or manually
  `UPDATE jobs SET status='FAILED', finished_at=now(), max_lifetime_exceeded=true,
  failure_message='manually expired' WHERE id='2158b850-8467-4088-8c53-c6fed7d0ca9f';`
  and FAIL/finish `data_source_imports` row `34ac14e1…`. A backend restart then
  starts a real import within one cron tick.
- Related: plan 104 (parallel per-source runs) is what these per-source
  `data_source_imports`/metadata rows come from; plan 90/88 expose
  `imported_until` and the last-import UI used here.
