# 54 - Align the overview loading skeletons with the rendered cards

Status: implemented

## Problem

The counting-station overview panel
([`StationOverview.tsx`](frontend/src/features/stationOverview/StationOverview.tsx))
shows inline `<Skeleton>` blocks while the shell and stats load. Those
"loading-ghosts" do not match the boxes that render once the data arrives:

- The all-time total ghost is a plain `h-20` block, but the real
  [`TotalBikesCard`](frontend/src/features/stationOverview/TotalBikesCard.tsx:6)
  is a bordered `bg-muted/40 p-4` card.
- The metrics ghost is a **single** `h-16` block, but **four**
  [`MetricCard`](frontend/src/features/stationOverview/MetricCard.tsx:17)
  boxes (`last_day` / `last_7_days` / `last_month` / `last_year`) render in a
  `gap-2` column.
- The shell ghost also omits the badge/updated row, so the layout jumps when
  the shell arrives.

As a result the skeleton shrinks/grows and reflows when content arrives.

## Goals

- Make the overview panel's loading placeholders mirror the exact rendered
  boxes: one bordered total card + four bordered metric cards in a single
  column with `gap-2`.
- Reuse the existing card-level skeletons instead of duplicating `MetricCard`'s
  internals.
- Keep the shell ghost aligned too (image + description + badge/updated row +
  the same stats skeleton).

## Changes

### 1. Share the card skeletons ([`Skeletons.tsx`](frontend/src/features/stationDetail/Skeletons.tsx))

- Export the existing private `MetricBoxSkeleton` (already sized/shaped like
  `MetricCard`), and tighten its right column from `gap-1` to `gap-0.5` to
  match `MetricCard`.
- Add an exported `TotalBikesSkeleton` mirroring `TotalBikesCard`: the same
  `rounded-md border bg-muted/40 p-4` shell with a label line and a value line
  inside.

### 2. Add a panel skeleton ([`stationOverview/Skeletons.tsx`](frontend/src/features/stationOverview/Skeletons.tsx))

New `OverviewPanelSkeleton` composed of `TotalBikesSkeleton` + a single-column
`ul` (`flex flex-col gap-2`) of four `MetricBoxSkeleton` entries — matching the
rendered `TotalBikesCard` + `<ul className="flex flex-col gap-2">` in
[`StationOverview.tsx`](frontend/src/features/stationOverview/StationOverview.tsx:107).

### 3. Wire it into [`StationOverview.tsx`](frontend/src/features/stationOverview/StationOverview.tsx)

- Replace the two misaligned inline skeletons in the shell-loading branch
  (`page === null`) with the image/description/badge ghosts +
  `OverviewPanelSkeleton`.
- Replace the two inline skeletons in the stats-loading branch
  (`stats === null`) with `OverviewPanelSkeleton`.

## Definition of done

- [x] `npm run build` green in [`frontend/`](frontend) (type-check + bundle)
- [x] `make test-playwright` green (frontend UI change)
