# 55 - Small UI + script fixes

Status: implemented

## Problem

A batch of small, unrelated polish items collected from a review pass:

1. Shell scripts and the Makefile use `cd`, which is considered unsafe (it
   changes the working directory for later commands if a step fails). `rm`
   calls are also scattered across [`scripts/e2e-playwright.sh`](../scripts/e2e-playwright.sh)
   and should be grouped so they can be reviewed manually.
2. `make test-playwright` takes a long time and prints almost nothing during the
   long build/wait phases.
3. The top header overflows / wraps badly on narrow screens; the global summary
   text must truncate on a **single line** instead of wrapping or disappearing.
4. The logo and "Bike Counter" text are not clickable.
5. The header search trigger is too narrow.
6. The search trigger must **not** carry the bike icon; instead each search
   **result row** should show the bike icon image (like the sidebar list items),
   which requires exposing `image_url` on the search endpoint.

## Goal

- Remove `cd` from the Makefile and [`scripts/e2e-playwright.sh`](../scripts/e2e-playwright.sh),
  replacing it with `--manifest-path` / `--prefix` / `readlink -f`.
- Group the temporary-file `rm` calls in the e2e script into one block.
- Print three short progress milestones ("built", "started", "finished") during
  the Playwright run.
- Make the header single-line and non-overflowing: truncate the global summary
  (ellipsis, one line), link the brand to the homepage, and widen the search
  trigger (keeping the magnifier icon).
- Add the station image (bike-icon fallback) to every search result row.

## Scope decision (revised after review)

- The bike icon belongs on each **search result** (station image, like the
  sidebar's [`SidebarListItem`](../frontend/src/features/sidebar/SidebarListItem.tsx)),
  **not** on the search trigger. The trigger keeps the `Search` magnifier icon.
- The global summary stays visible but truncates to one line with an ellipsis —
  it is never hidden and never wrapped to a second line.

## Approach

### 1. Makefile — remove `cd` (done)

Replace every `cd <dir> && <tool>` with the tool's directory flag so the working
directory never changes:

| Target | Before | After |
|---|---|---|
| `build` | `cd backend && cargo build --quiet` | `cargo build --manifest-path backend/Cargo.toml --quiet` |
| `fmt` | `cd backend && cargo fmt --quiet` | `cargo fmt --manifest-path backend/Cargo.toml --quiet` |
| `test` | `cd backend && cargo test --quiet` | `cargo test --manifest-path backend/Cargo.toml --quiet` |
| `test-rest` | `cd backend && cargo test --quiet adapter::driving::rest::tests` | `cargo test --manifest-path backend/Cargo.toml --quiet adapter::driving::rest::tests` |
| `clean` | `cd backend && cargo clean` | `cargo clean --manifest-path backend/Cargo.toml` |
| `playwright-install` | `cd frontend && npx playwright install chromium` | `npm exec --prefix frontend -- playwright install chromium` |
| `frontend-build` | `cd frontend && npm run build` | `npm run build --prefix frontend` |

### 2. e2e script — remove `cd`, group `rm`, add progress output (done)

In [`scripts/e2e-playwright.sh`](../scripts/e2e-playwright.sh):

- Replace the subshell path resolution:
  - `SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"`
    → `SCRIPT_DIR="$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")"`
  - `PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"`
    → `PROJECT_ROOT="$(dirname "${SCRIPT_DIR}")"`
- Replace `cd "${PROJECT_ROOT}/frontend"` with frontend-prefixed commands:
  - `npm ci --prefix "${PROJECT_ROOT}/frontend"`
  - `npm exec --prefix "${PROJECT_ROOT}/frontend" -- playwright install chromium`
  - `FRONTEND_URL=… npm exec --prefix "${PROJECT_ROOT}/frontend" -- playwright test --config "${PROJECT_ROOT}/frontend/playwright.config.ts"`
  - existence check → `[ ! -x "${PROJECT_ROOT}/frontend/node_modules/.bin/playwright" ]`.
- Group the temporary-file `rm` calls in `cleanup()` (`CONFIG_BACKUP`, `BUILD_LOG`)
  and drop the two mid-script `rm -f "${BUILD_LOG}"` calls; declare `BUILD_LOG=""`
  before the trap.
- Add the three milestones: `--- Stack built.`, `--- Stack started (app ready).`,
  `--- Tests finished.`.

### 3. Header — single line, truncated summary, brand link, wider trigger (redo)

In [`frontend/src/features/header/TopBar.tsx`](../frontend/src/features/header/TopBar.tsx):

1. Import `Link` from `react-router-dom`.
2. Wrap the logo + "Bike Counter" in
   `<Link to="/" className="flex min-w-0 items-center gap-2 font-bold whitespace-nowrap justify-self-start">`
   with the text span `truncate` so it ellipsizes only if absolutely necessary.
3. Keep the search trigger on a single centered column and widen it from
   `w-[26rem]` to `w-[32rem]` (keep `max-w-[60vw]`). Keep the `Search` magnifier
   icon; **do not** add the bike icon here.
4. Keep the header on **one line** (no wrapping). Change the grid to
   `grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)]` so the side columns can shrink
   to zero, and give the global-summary container `min-w-0` + `overflow-hidden`.
   Split the summary into two parts: the stats span (`truncate`, shrinks first)
   plus the "updated …" timestamp (`shrink-0`, always visible) — when space is
   tight only the timestamp remains, so the summary never overlaps the search
   bar and the most useful piece (the update time) is never truncated away.

Recommended markup shape:

```tsx
<header className="grid grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)] items-center gap-4 bg-primary px-4 py-2 text-primary-foreground shadow-md">
  <Link to="/" className="flex min-w-0 items-center gap-2 font-bold whitespace-nowrap justify-self-start">
    <img src="/bike-icon.svg" alt="" aria-hidden="true" className="h-8 w-8 shrink-0" />
    <span className="truncate">Bike Counter</span>
  </Link>

  <Button
    type="button"
    variant="ghost"
    onClick={onOpenSearch}
    className="w-[32rem] max-w-[60vw] justify-start gap-2 rounded-lg bg-white/15 px-3 py-2 text-left font-normal text-primary-foreground hover:bg-white/25 hover:text-primary-foreground"
  >
    <Search aria-hidden="true" />
    Search counting stations…
  </Button>

  <div className="flex min-w-0 items-center justify-end gap-3 justify-self-end overflow-hidden">
    {summary && (
      <span className="truncate text-sm text-primary-foreground/80">
        {summary.station_count} stations · {formatNumber(summary.channel_count)}{' '}
        channels · {formatNumber(summary.bikes_last_day_total)} bikes / last day · updated{' '}
        {formatTimestamp(summary.last_update)}
      </span>
    )}
    {error && <span className="truncate text-sm text-red-200">Global summary unavailable.</span>}
  </div>
</header>
```

### 4. Bike icon on each search result row (backend + frontend)

The search dialog renders [`StationListItem`](../frontend/src/features/stations/StationListItem.tsx),
which currently shows no image, while the sidebar's
[`SidebarListItem`](../frontend/src/features/sidebar/SidebarListItem.tsx) shows the station
image (the built-in bike icon is the fallback via `default_asset()`). Add the
same image to the search results:

**Backend**

- [`backend/src/adapter/driving/bff/dto.rs`](../backend/src/adapter/driving/bff/dto.rs:37):
  add `pub image_url: String` to `StationSummaryDto`; the existing
  `From<StationSummary>` impl sets it to `String::new()` as a placeholder (the
  handler populates it below).
- [`backend/src/adapter/driving/bff/handlers.rs`](../backend/src/adapter/driving/bff/handlers.rs:346)
  `get_bff_stations_search`: collect the stations from the summaries, resolve
  their image URLs with the existing
  [`station_image_urls`](../backend/src/adapter/driving/bff/handlers.rs:150) batch
  helper, then fill `image_url` on each DTO (keyed by `dto.station.id`), e.g.:

  ```rust
  let summaries = blocking(move || service.summaries(None, now)).await.map_err(map_domain_error)?;
  let stations: Vec<CountingStation> = summaries.iter().map(|s| s.station.clone()).collect();
  let image_urls = station_image_urls(&state, &stations).await?;
  let mut items: Vec<StationSummaryDto> = summaries.into_iter().map(StationSummaryDto::from).collect();
  for dto in &mut items {
      dto.image_url = image_urls.get(&dto.station.id).cloned().unwrap_or_default();
  }
  ```

- Update the search endpoint test in
  [`backend/src/adapter/driving/rest/tests/bff.rs`](../backend/src/adapter/driving/rest/tests/bff.rs:226)
  to assert `image_url` on each item (the default built-in asset path).

**Frontend**

- [`frontend/src/features/stations/types.ts`](../frontend/src/features/stations/types.ts:14):
  add `image_url: string` to `StationSummary`.
- [`frontend/src/features/stations/StationListItem.tsx`](../frontend/src/features/stations/StationListItem.tsx:9):
  render the station image as the leading thumbnail, mirroring the sidebar item:
  `<img src={station.image_url} alt="" className="h-12 w-12 shrink-0 rounded-md border object-cover" />`
  inside the select button, next to the name/description/stats. Move the hover
  highlight to the `<li>` (`hover:bg-accent` + `transition-colors`) and neutralize
  the button's own hover (`hover:bg-transparent`) so the whole row highlights.

### 5. Verification

- `make check` — fmt + clippy.
- `make test-rest` — includes the updated search endpoint test.
- `npm run build --prefix frontend` — tsc + vite.
- Manual: narrow the browser window and confirm the header stays on one line with
  the summary ellipsizing; confirm each search result shows the bike-icon image
  and the trigger shows only the magnifier.

## Definition of done

- [x] Makefile no longer uses `cd`; targets still work.
- [x] e2e script no longer uses `cd`; `rm` calls grouped in `cleanup()`; three
      milestone lines print.
- [x] Header links the brand to `/`, widens the search trigger (magnifier only),
      and truncates the global summary on one line without wrapping.
- [x] Search results show the station image (bike-icon fallback) via `image_url`.
- [x] `make check`, `make test-rest` and a frontend build are green.
