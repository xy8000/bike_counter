# 77 - Small frontend fixes + "last updated" investigation

Status: implemented

## Follow-up (review feedback)

- **Sidebar handle is a side extension**: the pull/push handle is attached to
  the panel's right edge and extends fully outward (`right-0 translate-x-full`),
  like a tab growing out of the sidebar's side; it is rounded only on the right
  (`rounded-r-lg`, `border-l-0`) so its left edge reads as seamless with the
  panel. Collapsed the panel slides fully off-screen (`-translate-x-full`), so
  the tab alone remains as the pull handle at the map's left edge.
- **Generic left panel**: the pull/push handle was only rendered with the
  station list. The station list and the station overview now live inside one
  shared sliding [`LeftPanel`](../frontend/src/features/sidebar/LeftPanel.tsx:1)
  (with the shared [`SidebarHandle`](../frontend/src/features/sidebar/SidebarHandle.tsx:1)
  on its right edge). The handle collapses/expands WHATEVER content is currently
  shown, and the panel re-opens with the same content it had when closed —
  so closing the sidebar while the overview is open collapses the overview, and
  pulling re-opens it. Covered by two new Playwright specs:
  [`map.spec.ts`](../frontend/e2e/map.spec.ts:96) (collapse while the overview is
  open, pull re-opens the overview) and
  [`sidebar.spec.ts`](../frontend/e2e/sidebar.spec.ts:69) (a collapsed panel
  re-opens onto the overview when a station is selected).
- **Settings-dialog e2e**: [`settings.spec.ts`](../frontend/e2e/settings.spec.ts:84)
  enables and disables the switch and asserts the example graph switches between
  "All stations" (new-station bikes stacked, recent months jump) and "Established
  only" (new station excluded, totals grow more evenly).
- **Switch label layout**: the shadcn [`Label`](../frontend/src/components/ui/label.tsx:11)
  defaults to `flex items-center`, which laid the title and explanation out side
  by side. The label is now `flex-col items-start gap-0.5` with the title in a
  `whitespace-nowrap` span, so "Exclude new stations from trends" stays on one
  line with the explanation rendered below it.
- **Settings illustration**: a single chart that SWITCHES between the two states
  when the setting toggles (no side-by-side mutation): off shows all stations
  with a new station's bikes stacked on top (`NEW_STATION`), so recent months
  jump (e.g. 1,2,6,7,9,10); on shows the established-only version with that
  station left out, so the totals grow more evenly (e.g. 1,2,3,4,5,7,8,9). This
  matches the backend:
  [`stations_summary_monthly`](../backend/src/core/application/station_analytics/service.rs:665)
  + [`established_stations_for_year`](../backend/src/core/application/station_analytics/service.rs:194)
  (a station must cover the whole current + previous year to stay in). The dialog
  is widened to `sm:max-w-2xl` and captions wrap, so the text is no longer
  truncated; no "baseline" framing is used.

- **Schema finding**: the plan's original assumption (an existing-but-unused
  `data_sources.last_updated_at` column) was wrong — migration
  [`V7__rename_data_source_imported_until.sql`](../backend/migrations/V7__rename_data_source_imported_until.sql:3)
  **renamed** the original `last_updated_at` column to `imported_until`. The fix
  therefore adds a fresh column via
  [`V16__add_data_source_last_updated.sql`](../backend/migrations/V16__add_data_source_last_updated.sql:1).
  The header timestamp now prefers the newest per-source `last_updated_at` (so a
  partial multi-source run where one city fails no longer shows "never") and
  falls back to the newest finished update job's `finished_at` for legacy/seed
  data (keeps the e2e seed and REST fixtures working unchanged).
- **Sidebar slide**: Tailwind v4 `translate-x-*` uses the CSS `translate`
  property (not `transform`), and `calc(100% - 1.5rem)` needs spaces around the
  operator — the arbitrary-value class is written `-translate-x-[calc(100%_-_1.5rem)]`.
  The Playwright spec asserts the computed `translate` is negative instead of
  reading `transform`.
- **Settings control**: the checkbox is now a `Switch` (adds
  `@radix-ui/react-switch`), so the e2e specs locate it via `getByRole('switch')`
  and toggle with `.click()`.

## Problem

Three small frontend nits plus one suspected data-path error:

- **A — Bike-Trends settings are too small and bare.** The settings dialogue is a
  single checkbox in a narrow `sm:max-w-md` dialog; it should be larger and give
  the user a visual idea of what "exclude new stations" actually does.
- **B — Sidebar collapse feels wrong.** The expanded sidebar hides behind a
  header close button, the collapsed state is a separate slim bar, and the
  transition animates the panel *width* instead of sliding the full panel in/out.
- **C — "Last updated" shows "never".** The top-right header renders
  `updated never` whenever the global summary reports `last_update = null`, and
  the user suspects this is a data-path error rather than a pure UI wording
  issue.

## Investigation findings (part C)

The header timestamp is fed by [`formatTimestamp`](../frontend/src/lib/format.ts:11),
which returns `never` for `null`. The backend value comes from
[`metrics::last_update`](../backend/src/core/application/station_analytics/metrics.rs:41),
which is the `finished_at` of the **most recent FINISHED `data_source_update`
job** ([`find_last_finished_by_type`](../backend/src/adapter/driven/postgres/job_repository.rs:217)).

That job is a single coarse job covering **all** configured data sources
([`run_updates`](../backend/src/core/application/data_source_update_service.rs:217)):
it loops over Münster, Bonn and Hamburg in order and returns `Err` on the first
source that fails, after which
[`execute`](../backend/src/core/application/data_source_update_service.rs:185)
marks the whole job `FAILED`. Consequently, if **any one** source fails (or the
job is still running), there is no `FINISHED` job at all, so the header shows
"never" even though the earlier sources imported fine and their data is visible
on the map.

Two secondary correctness issues were also found:

- [`find_last_finished_by_type`](../backend/src/adapter/driven/postgres/job_repository.rs:225)
  orders by `created_at DESC`, but the documented semantics is "most recent
  successful update", which should order by `finished_at DESC`.
- The original `data_sources.last_updated_at` column (added in
  [`V3__add_jobs.sql`](../backend/migrations/V3__add_jobs.sql:25)) was **renamed
  to `imported_until`** by
  [`V7__rename_data_source_imported_until.sql`](../backend/migrations/V7__rename_data_source_imported_until.sql:3)
  and repurposed as the import watermark. There is therefore no per-source
  wall-clock "last updated" column today; the fix adds a fresh one via a new
  migration (V16).

**Root cause to confirm at implementation time** (needs the running stack):

```bash
docker compose exec db psql -U postgres -d bike_counter \
  -c "SELECT job_type, status, created_at, started_at, finished_at, failure_message
      FROM jobs WHERE job_type = 'data_source_update' ORDER BY created_at DESC LIMIT 20;"
```

Expected confirmation: no `FINISHED` rows (only `FAILED`/`RUNNING`/`PENDING`),
while `counting_stations`/`measurements` clearly contain data.

## Decisions

### A — Settings dialogue

- Widen the dialog to `sm:max-w-xl` and lay the control out with a **switch**
  instead of a checkbox, following the shadcn pattern (new
  `components/ui/switch.tsx` backed by `@radix-ui/react-switch`).
- Add a self-contained, decorative inline illustration (CSS divs, no external
  image) that shows the difference between "all stations" and "established only":
  a small grouped-bar comparison where the "new" station bar is grayed out and
  hatched when the setting is on, and solid when it is off. The illustration
  mirrors the live `excludeNewStations` state.
- Keep the accessible name "Exclude new stations from trends" and the
  `exclude-new-stations` id so the localStorage/context behaviour is unchanged.

### B — Sidebar

- Replace the two separate `<aside>` variants with **one** always-rendered
  `<aside>` of fixed `w-[360px]` that slides horizontally via
  `transition-transform` (no `transition-[width]`):
  - expanded: `translate-x-0`
  - collapsed: `-translate-x-[calc(100%-1.5rem)]` (only the 1.5rem handle stays
    visible at the map's left edge).
- Remove the header close button. Add a single pull/push handle button pinned at
  the vertical middle of the panel's right edge (`top-1/2 -translate-y-1/2`),
  icon `ChevronLeft` when expanded and `ChevronRight` when collapsed, with the
  existing accessible names `Hide station list` / `Show station list` (both keep
  the `(H)` title). The same button stays `role="button"` so the existing
  [`map.spec.ts`](../frontend/e2e/map.spec.ts:25) collapse step keeps working.
- The handle is the only visible part when collapsed; the rest of the panel sits
  off-screen to the left, so the map is not "pushed" — it looks pulled out from
  under the edge.

### C — Last updated

- Primary fix: make the header timestamp reflect **per-data-source** success
  rather than the all-or-nothing job outcome.
  1. Maintain the existing `data_sources.last_updated_at` column: extend the
     [`DataSourceRepository`](../backend/src/core/domain/data_source/repository_port.rs:19)
     port with `update_last_updated(id, now)`, map the column in
     [`data_source_repository.rs`](../backend/src/adapter/driven/postgres/data_source_repository.rs:18)
     and write it after each source's successful update in
     [`run_updates`](../backend/src/core/application/data_source_update_service.rs:217).
  2. Compute the global `last_update` from the newest
     `data_sources.last_updated_at` (falling back to `None` when no source has
     ever succeeded) instead of the job table, wiring the
     `DataSourceRepository` through
     [`StationAnalyticsService`](../backend/src/core/application/station_analytics/service.rs:1)
     and [`metrics::last_update`](../backend/src/core/application/station_analytics/metrics.rs:41).
  3. Fix the ordering bug in
     [`find_last_finished_by_type`](../backend/src/adapter/driven/postgres/job_repository.rs:225)
     (`ORDER BY finished_at DESC NULLS LAST`) so the job-based scheduler logic is
     also correct.
- If the investigation instead reveals a single flaky provider as the only
  blocker, keep the per-source fix anyway (it is the root-cause-level correction)
  and separately note the provider issue.

## Changes

### Frontend

- New [`components/ui/switch.tsx`](../frontend/src/components/ui): shadcn-style
  `Switch` using `@radix-ui/react-switch` (add the dependency via
  `npm install @radix-ui/react-switch` in `frontend/`; `package-lock.json`
  updates accordingly).
- [`features/settings/SettingsDialog.tsx`](../frontend/src/features/settings/SettingsDialog.tsx:26):
  widen to `sm:max-w-xl`, swap checkbox for switch, add the inline comparison
  illustration (local component, `aria-hidden`), keep the existing heading and
  description.
- [`features/sidebar/Sidebar.tsx`](../frontend/src/features/sidebar/Sidebar.tsx:54):
  single sliding `<aside>` + mid-height pull/push handle as described above.
- [`features/header/TopBar.tsx`](../frontend/src/features/header/TopBar.tsx:34):
  no functional change expected after the backend fix (the timestamp keeps its
  `updated …` rendering); only revisit if the investigation wants a fallback
  label for the true "no source ever succeeded" state.

### Backend

- New migration
  [`V16__add_data_source_last_updated.sql`](../backend/migrations/V16__add_data_source_last_updated.sql:1):
  `ALTER TABLE data_sources ADD COLUMN last_updated_at TIMESTAMPTZ;` (re-establishes
  the per-source wall-clock marker after the V7 rename).
- [`domain/data_source/repository_port.rs`](../backend/src/core/domain/data_source/repository_port.rs:19):
  add `update_last_updated(id, now)`.
- [`adapter/driven/postgres/data_source_repository.rs`](../backend/src/adapter/driven/postgres/data_source_repository.rs:18):
  map `last_updated_at` in `map_row`, implement `update_last_updated`, and add it
  to the read columns.
- [`core/application/data_source_update_service.rs`](../backend/src/core/application/data_source_update_service.rs:217):
  after each source succeeds and its `imported_until` advances, also call
  `update_last_updated(id, now)`.
- [`core/application/station_analytics/metrics.rs`](../backend/src/core/application/station_analytics/metrics.rs:41):
  change `last_update` to read the newest per-source `last_updated_at` (accept the
  data-source repository), keeping the job-repository path as fallback if
  preferred during implementation.
- [`core/application/station_analytics/service.rs`](../backend/src/core/application/station_analytics/service.rs:375):
  pass the data-source repository through; update `global_summary`, overview
  shell, detail page and stations-summary `last_update` call sites.
- [`main.rs`](../backend/src/main.rs:237): pass `data_source_repo` into
  `StationAnalyticsService::new`.
- [`adapter/driven/postgres/job_repository.rs`](../backend/src/adapter/driven/postgres/job_repository.rs:225):
  order `find_last_finished_by_type` by `finished_at DESC NULLS LAST`.
- Tests: update all `DataSourceRepository` mocks (the service tests in
  [`data_source_service.rs`](../backend/src/core/application/data_source_service.rs:50),
  [`startup_service.rs`](../backend/src/core/application/startup_service.rs:309),
  [`provider_message_service.rs`](../backend/src/core/application/provider_message_service.rs:156),
  [`persistent_state_service.rs`](../backend/src/core/application/persistent_state_service.rs:195),
  [`data_source_update_service.rs`](../backend/src/core/application/data_source_update_service.rs:587)
  and [`rest/tests/mocks.rs`](../backend/src/adapter/driving/rest/tests/mocks.rs:403));
  extend the station-analytics tests
  ([`tests.rs`](../backend/src/core/application/station_analytics/tests.rs:1)) with
  "newest per-source last-update wins even without a finished job", and add a
  job-repository test for the `finished_at` ordering.

### Docs / gates

- Register this plan in [`plans/README.md`](../plans/README.md:13).
- Playwright: update
  [`settings.spec.ts`](../frontend/e2e/settings.spec.ts:32) to locate the
  `role="switch"` and toggle via `.click()` (keep `toBeChecked` assertions); add a
  sidebar-handle scenario that collapses via `Hide station list`, asserts the
  `Show station list` handle is visible mid-panel, and re-expands.
- `make check`, `make test-rest` / `make test`, `make coverage`,
  `make test-playwright` all green.

## Flow

```mermaid
flowchart LR
    A[Settings dialog] --> A1[wider sm:max-w-xl]
    A1 --> A2[switch control]
    A2 --> A3[inline all-vs-established illustration]

    B[Sidebar] --> B1[single fixed-width aside]
    B1 --> B2[translate-x slide animation]
    B2 --> B3[mid-height pull/push handle]

    C[Header updated never] --> C1[inspect jobs table]
    C1 --> C2[per-source last_updated_at]
    C2 --> C3[global summary reads newest source]
</mermaid>
```
